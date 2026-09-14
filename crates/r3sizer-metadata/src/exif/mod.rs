//! EXIF correction: bounded TIFF structural validation and patching.
//!
//! `correct()` is the only entry point used by the rest of the crate. It
//! never partially patches an ambiguous or malformed structure -- either
//! the whole corrected TIFF block comes back, or `None` does, with the
//! reason recorded as a single issue. See `reader.rs` for the validating
//! parser (structure only, no tag semantics) and `patch.rs` for the
//! tag-by-tag correction policy and the actual byte patching.

mod patch;
mod reader;

use crate::limits::MetadataLimits;
use crate::types::{MetadataIssue, OutputFacts};

/// Correct dimension, orientation, and color-space EXIF fields in `raw`
/// (TIFF bytes starting at the byte-order marker, as produced by the
/// container scanners in Task 2) to match `facts`, within `limits`.
///
/// Returns `None` when the block can't be safely corrected at all (bad
/// header, BigTIFF, a cycle, a duplicate tag, an aliasing table/value
/// region, or any other structural inconsistency) -- the caller should omit
/// the EXIF block entirely rather than embed unverified bytes.
pub fn correct(
    raw: &[u8],
    facts: &OutputFacts,
    limits: &MetadataLimits,
) -> (Option<Vec<u8>>, Vec<MetadataIssue>) {
    let parsed = match reader::parse(raw, limits) {
        Ok(p) => p,
        Err(issue) => return (None, vec![issue]),
    };
    match patch::apply(raw, &parsed, facts) {
        Ok((bytes, issues)) => (Some(bytes), issues),
        Err(issue) => (None, vec![issue]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ColorAction, MetadataCategory, MetadataIssueReason, OrientationAction};

    /// TIFF-6.0-style IFD builder for tests: `entries` are
    /// `(tag, type, count, value)` where `value` is the raw 4-byte
    /// value/offset field content as a u32 (matching how small-value
    /// fixtures are naturally written).
    fn build_ifd(little_endian: bool, entries: &[(u16, u16, u32, u32)], next_ifd: u32) -> Vec<u8> {
        let mut b = Vec::new();
        if little_endian {
            b.extend(b"II\x2a\0");
        } else {
            b.extend(b"MM\0\x2a");
        }
        let ifd0_offset: u32 = 8;
        push_u32(&mut b, ifd0_offset, little_endian);
        push_u16(&mut b, entries.len() as u16, little_endian);
        for &(tag, kind, count, value) in entries {
            push_u16(&mut b, tag, little_endian);
            push_u16(&mut b, kind, little_endian);
            push_u32(&mut b, count, little_endian);
            // Inline values are left-justified in the 4-byte field for both
            // byte orders: a SHORT's 2 encoded bytes go first, padded with
            // zeros, never just the low/high half of a 4-byte encoding of
            // `value` (which only coincides with left-justification for
            // little-endian). Out-of-line entries (this helper's tests
            // don't build any directly with a narrower type) and LONGs use
            // the full 4 bytes either way.
            if kind == 3 && count == 1 {
                // SHORT
                let mut field = [0u8; 4];
                if little_endian {
                    field[0..2].copy_from_slice(&(value as u16).to_le_bytes());
                } else {
                    field[0..2].copy_from_slice(&(value as u16).to_be_bytes());
                }
                b.extend(field);
            } else {
                push_u32(&mut b, value, little_endian);
            }
        }
        push_u32(&mut b, next_ifd, little_endian);
        b
    }

    fn push_u16(b: &mut Vec<u8>, v: u16, little_endian: bool) {
        if little_endian {
            b.extend(v.to_le_bytes());
        } else {
            b.extend(v.to_be_bytes());
        }
    }

    fn push_u32(b: &mut Vec<u8>, v: u32, little_endian: bool) {
        if little_endian {
            b.extend(v.to_le_bytes());
        } else {
            b.extend(v.to_be_bytes());
        }
    }

    fn tiny_exif() -> Vec<u8> {
        let mut b = b"II\x2a\0\x08\0\0\0".to_vec();
        b.extend(3u16.to_le_bytes());
        for (tag, kind, value) in [(0x0100u16, 4u16, 400u32), (0x0101, 4, 200), (0x0112, 3, 6)] {
            b.extend(tag.to_le_bytes());
            b.extend(kind.to_le_bytes());
            b.extend(1u32.to_le_bytes());
            b.extend(value.to_le_bytes());
        }
        b.extend(0u32.to_le_bytes());
        b
    }

    fn default_facts(orientation: OrientationAction, color: ColorAction) -> OutputFacts {
        OutputFacts {
            width: 100,
            height: 50,
            orientation,
            color,
        }
    }

    // Keep this test inside exif/mod.rs so it can call the private correct().
    #[test]
    fn updates_dimensions_and_normalizes_only_on_request() {
        let facts = OutputFacts {
            width: 100,
            height: 50,
            orientation: OrientationAction::Normalize,
            color: ColorAction::Unverified,
        };
        let (bytes, issues) = correct(&tiny_exif(), &facts, &MetadataLimits::default());
        assert!(issues.is_empty());
        let exif = ::exif::Reader::new().read_raw(bytes.unwrap()).unwrap();
        assert_eq!(
            exif.get_field(::exif::Tag::ImageWidth, ::exif::In::PRIMARY)
                .unwrap()
                .value
                .get_uint(0),
            Some(100)
        );
        assert_eq!(
            exif.get_field(::exif::Tag::Orientation, ::exif::In::PRIMARY)
                .unwrap()
                .value
                .get_uint(0),
            Some(1)
        );
    }

    #[test]
    fn preserve_keeps_original_orientation() {
        let facts = default_facts(OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(&tiny_exif(), &facts, &MetadataLimits::default());
        assert!(issues.is_empty());
        let exif = ::exif::Reader::new().read_raw(bytes.unwrap()).unwrap();
        assert_eq!(
            exif.get_field(::exif::Tag::Orientation, ::exif::In::PRIMARY)
                .unwrap()
                .value
                .get_uint(0),
            Some(6)
        );
    }

    #[test]
    fn big_endian_is_supported() {
        let ifd = build_ifd(
            false,
            &[(0x0100, 4, 1, 400), (0x0101, 4, 1, 200), (0x0112, 3, 1, 3)],
            0,
        );
        let facts = default_facts(OrientationAction::Normalize, ColorAction::Unchanged);
        let (bytes, issues) = correct(&ifd, &facts, &MetadataLimits::default());
        assert!(issues.is_empty(), "{issues:?}");
        let exif = ::exif::Reader::new().read_raw(bytes.unwrap()).unwrap();
        assert_eq!(
            exif.get_field(::exif::Tag::ImageWidth, ::exif::In::PRIMARY)
                .unwrap()
                .value
                .get_uint(0),
            Some(100)
        );
        assert_eq!(
            exif.get_field(::exif::Tag::Orientation, ::exif::In::PRIMARY)
                .unwrap()
                .value
                .get_uint(0),
            Some(1)
        );
    }

    #[test]
    fn all_eight_orientations_preserve_and_normalize() {
        for original in 1u32..=8 {
            let ifd = build_ifd(true, &[(0x0112, 3, 1, original)], 0);

            let preserve_facts = default_facts(OrientationAction::Preserve, ColorAction::Unchanged);
            let (bytes, issues) = correct(&ifd, &preserve_facts, &MetadataLimits::default());
            assert!(issues.is_empty());
            let exif = ::exif::Reader::new().read_raw(bytes.unwrap()).unwrap();
            assert_eq!(
                exif.get_field(::exif::Tag::Orientation, ::exif::In::PRIMARY)
                    .unwrap()
                    .value
                    .get_uint(0),
                Some(original)
            );

            let normalize_facts =
                default_facts(OrientationAction::Normalize, ColorAction::Unchanged);
            let (bytes, issues) = correct(&ifd, &normalize_facts, &MetadataLimits::default());
            assert!(issues.is_empty());
            let exif = ::exif::Reader::new().read_raw(bytes.unwrap()).unwrap();
            assert_eq!(
                exif.get_field(::exif::Tag::Orientation, ::exif::In::PRIMARY)
                    .unwrap()
                    .value
                    .get_uint(0),
                Some(1)
            );
        }
    }

    #[test]
    fn invalid_orientation_value_is_dropped_as_malformed() {
        let ifd = build_ifd(true, &[(0x0112, 3, 1, 9)], 0);
        let facts = default_facts(OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(&ifd, &facts, &MetadataLimits::default());
        let exif = ::exif::Reader::new().read_raw(bytes.unwrap()).unwrap();
        assert!(exif
            .get_field(::exif::Tag::Orientation, ::exif::In::PRIMARY)
            .is_none());
        assert!(issues.iter().any(|i| {
            i.category == MetadataCategory::Exif
                && i.reason == MetadataIssueReason::Malformed
                && i.field.as_deref() == Some("orientation")
        }));
    }

    #[test]
    fn short_dimension_promotes_to_long_when_it_no_longer_fits() {
        // ImageWidth stored as SHORT (type 3); requested output width
        // exceeds u16::MAX, forcing a promotion to LONG in the same entry.
        let ifd = build_ifd(true, &[(0x0100, 3, 1, 1000)], 0);
        let facts = OutputFacts {
            width: 70_000,
            height: 50,
            orientation: OrientationAction::Preserve,
            color: ColorAction::Unchanged,
        };
        let (bytes, issues) = correct(&ifd, &facts, &MetadataLimits::default());
        assert!(issues.is_empty(), "{issues:?}");
        let exif = ::exif::Reader::new().read_raw(bytes.unwrap()).unwrap();
        assert_eq!(
            exif.get_field(::exif::Tag::ImageWidth, ::exif::In::PRIMARY)
                .unwrap()
                .value
                .get_uint(0),
            Some(70_000)
        );
    }

    #[test]
    fn short_dimension_stays_short_when_it_still_fits() {
        let ifd = build_ifd(true, &[(0x0100, 3, 1, 1000)], 0);
        let facts = default_facts(OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(&ifd, &facts, &MetadataLimits::default());
        assert!(issues.is_empty());
        let exif = ::exif::Reader::new().read_raw(bytes.unwrap()).unwrap();
        assert_eq!(
            exif.get_field(::exif::Tag::ImageWidth, ::exif::In::PRIMARY)
                .unwrap()
                .value
                .get_uint(0),
            Some(100)
        );
    }

    #[test]
    fn maker_note_is_removed() {
        // ExifIFD with a MakerNote (0x927c, UNDEFINED, out-of-line blob).
        let mut b = b"II\x2a\0\x08\0\0\0".to_vec();
        push_u16(&mut b, 1, true);
        push_u16(&mut b, 0x8769, true); // ExifIFD pointer
        push_u16(&mut b, 4, true); // LONG
        push_u32(&mut b, 1, true);
        let exif_ifd_offset_pos = b.len();
        push_u32(&mut b, 0, true); // placeholder, patched below
        push_u32(&mut b, 0, true); // next IFD = 0

        let exif_ifd_offset = b.len() as u32;
        b[exif_ifd_offset_pos..exif_ifd_offset_pos + 4]
            .copy_from_slice(&exif_ifd_offset.to_le_bytes());

        let maker_note_data = vec![0xABu8; 20];
        push_u16(&mut b, 1, true); // 1 entry in ExifIFD
        push_u16(&mut b, 0x927c, true); // MakerNote
        push_u16(&mut b, 7, true); // UNDEFINED
        push_u32(&mut b, maker_note_data.len() as u32, true);
        let maker_note_offset_pos = b.len();
        push_u32(&mut b, 0, true); // placeholder
        push_u32(&mut b, 0, true); // next IFD = 0

        let maker_note_offset = b.len() as u32;
        b[maker_note_offset_pos..maker_note_offset_pos + 4]
            .copy_from_slice(&maker_note_offset.to_le_bytes());
        b.extend(&maker_note_data);

        let facts = default_facts(OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(&b, &facts, &MetadataLimits::default());
        let bytes = bytes.expect("structurally valid, should not be omitted");
        assert!(issues.iter().any(|i| {
            i.category == MetadataCategory::MakerNote && i.reason == MetadataIssueReason::Unverified
        }));
        // MakerNote bytes are zeroed in the output.
        let start = maker_note_offset as usize;
        assert!(bytes[start..start + maker_note_data.len()]
            .iter()
            .all(|&b| b == 0));
        let exif = ::exif::Reader::new().read_raw(bytes).unwrap();
        assert!(exif
            .get_field(::exif::Tag::MakerNote, ::exif::In::PRIMARY)
            .is_none());
    }

    #[test]
    fn embedded_preview_pointer_is_removed() {
        let ifd = build_ifd(true, &[(0x0201, 4, 1, 1234), (0x0202, 4, 1, 100)], 0);
        let facts = default_facts(OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(&ifd, &facts, &MetadataLimits::default());
        assert!(bytes.is_some());
        assert_eq!(
            issues
                .iter()
                .filter(|i| i.category == MetadataCategory::Thumbnail
                    && i.reason == MetadataIssueReason::RemovedStale
                    && i.field.as_deref() == Some("preview"))
                .count(),
            2
        );
    }

    #[test]
    fn thumbnail_chain_is_detached_and_zeroed() {
        // IFD0 (no entries) with a next-IFD pointing at a thumbnail IFD
        // that itself holds one SHORT entry (inline, nothing out-of-line
        // to worry about beyond the table itself).
        let mut b = b"II\x2a\0\x08\0\0\0".to_vec();
        push_u16(&mut b, 0, true); // 0 entries in IFD0
        let next_pos = b.len();
        push_u32(&mut b, 0, true); // placeholder next-IFD offset

        let thumb_offset = b.len() as u32;
        b[next_pos..next_pos + 4].copy_from_slice(&thumb_offset.to_le_bytes());
        push_u16(&mut b, 1, true);
        push_u16(&mut b, 0x0100, true);
        push_u16(&mut b, 3, true);
        push_u32(&mut b, 1, true);
        push_u32(&mut b, 64, true);
        push_u32(&mut b, 0, true); // next = 0

        let facts = default_facts(OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(&b, &facts, &MetadataLimits::default());
        let bytes = bytes.expect("valid structure");
        assert!(issues.iter().any(|i| {
            i.category == MetadataCategory::Thumbnail
                && i.reason == MetadataIssueReason::RemovedStale
                && i.field.as_deref() == Some("thumbnail_chain")
        }));
        // The (former) thumbnail IFD table bytes are now all zero.
        let thumb_span = thumb_offset as usize..thumb_offset as usize + 2 + 12 + 4;
        assert!(bytes[thumb_span].iter().all(|&b| b == 0));
        // IFD0's own next-IFD pointer is now zero (chain detached).
        assert_eq!(&bytes[next_pos..next_pos + 4], &[0, 0, 0, 0]);
    }

    #[test]
    fn cycle_is_rejected() {
        // IFD0's next-IFD pointer points back at IFD0's own offset (8).
        let ifd = build_ifd(true, &[], 8);
        let facts = default_facts(OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(&ifd, &facts, &MetadataLimits::default());
        assert!(bytes.is_none());
        assert!(issues
            .iter()
            .any(|i| i.reason == MetadataIssueReason::Malformed
                && i.field.as_deref() == Some("cycle")));
    }

    #[test]
    fn overlapping_value_and_table_is_rejected() {
        // An ASCII entry (out-of-line, needs 10 bytes) whose declared
        // offset points squarely into IFD0's own entry table -- classic
        // aliasing attempt.
        let ifd = build_ifd(true, &[(0x010e, 2, 10, 10)], 0);
        let facts = default_facts(OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(&ifd, &facts, &MetadataLimits::default());
        assert!(bytes.is_none());
        assert!(issues
            .iter()
            .any(|i| i.reason == MetadataIssueReason::Malformed
                && i.field.as_deref() == Some("aliasing")));
    }

    #[test]
    fn duplicate_tag_is_rejected_as_malformed() {
        let ifd = build_ifd(true, &[(0x0100, 4, 1, 100), (0x0100, 4, 1, 200)], 0);
        let facts = default_facts(OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(&ifd, &facts, &MetadataLimits::default());
        assert!(bytes.is_none());
        assert!(issues
            .iter()
            .any(|i| i.reason == MetadataIssueReason::Malformed
                && i.field.as_deref() == Some("duplicate_tag")));
    }

    #[test]
    fn malformed_count_does_not_panic_and_is_rejected() {
        // count = u32::MAX with an 8-byte-wide type overflows count*width.
        let ifd = build_ifd(true, &[(0x9286, 7, u32::MAX, 0)], 0);
        let facts = default_facts(OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, _issues) = correct(&ifd, &facts, &MetadataLimits::default());
        // The entry itself is dropped as invalid; the rest of the (empty
        // otherwise) IFD is still fine to keep.
        assert!(bytes.is_some());
    }

    #[test]
    fn exif_ifd_dimensions_are_updated() {
        let mut b = b"II\x2a\0\x08\0\0\0".to_vec();
        push_u16(&mut b, 1, true);
        push_u16(&mut b, 0x8769, true);
        push_u16(&mut b, 4, true);
        push_u32(&mut b, 1, true);
        let exif_ifd_offset_pos = b.len();
        push_u32(&mut b, 0, true);
        push_u32(&mut b, 0, true);

        let exif_ifd_offset = b.len() as u32;
        b[exif_ifd_offset_pos..exif_ifd_offset_pos + 4]
            .copy_from_slice(&exif_ifd_offset.to_le_bytes());
        push_u16(&mut b, 2, true);
        push_u16(&mut b, 0xa002, true);
        push_u16(&mut b, 4, true);
        push_u32(&mut b, 1, true);
        push_u32(&mut b, 4000, true);
        push_u16(&mut b, 0xa003, true);
        push_u16(&mut b, 4, true);
        push_u32(&mut b, 1, true);
        push_u32(&mut b, 3000, true);
        push_u32(&mut b, 0, true);

        let facts = default_facts(OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(&b, &facts, &MetadataLimits::default());
        assert!(issues.is_empty(), "{issues:?}");
        let exif = ::exif::Reader::new().read_raw(bytes.unwrap()).unwrap();
        assert_eq!(
            exif.get_field(::exif::Tag::PixelXDimension, ::exif::In::PRIMARY)
                .unwrap()
                .value
                .get_uint(0),
            Some(100)
        );
        assert_eq!(
            exif.get_field(::exif::Tag::PixelYDimension, ::exif::In::PRIMARY)
                .unwrap()
                .value
                .get_uint(0),
            Some(50)
        );
    }

    #[test]
    fn rational_gps_and_capture_settings_survive_at_original_offset() {
        // GPSLatitude (3 RATIONALs = 24 bytes, out-of-line) inside a GPS
        // IFD reached from IFD0.
        let mut b = b"II\x2a\0\x08\0\0\0".to_vec();
        push_u16(&mut b, 1, true);
        push_u16(&mut b, 0x8825, true); // GPSIFD pointer
        push_u16(&mut b, 4, true);
        push_u32(&mut b, 1, true);
        let gps_ifd_offset_pos = b.len();
        push_u32(&mut b, 0, true);
        push_u32(&mut b, 0, true);

        let gps_ifd_offset = b.len() as u32;
        b[gps_ifd_offset_pos..gps_ifd_offset_pos + 4]
            .copy_from_slice(&gps_ifd_offset.to_le_bytes());
        push_u16(&mut b, 1, true);
        push_u16(&mut b, 0x0002, true); // GPSLatitude
        push_u16(&mut b, 5, true); // RATIONAL
        push_u32(&mut b, 3, true);
        let value_offset_pos = b.len();
        push_u32(&mut b, 0, true);
        push_u32(&mut b, 0, true); // next = 0

        let value_offset = b.len() as u32;
        b[value_offset_pos..value_offset_pos + 4].copy_from_slice(&value_offset.to_le_bytes());
        let rationals: [u32; 6] = [10, 1, 20, 1, 30, 1];
        for v in rationals {
            push_u32(&mut b, v, true);
        }

        let original_value_bytes = b[value_offset as usize..value_offset as usize + 24].to_vec();
        let facts = default_facts(OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(&b, &facts, &MetadataLimits::default());
        assert!(issues.is_empty(), "{issues:?}");
        let bytes = bytes.unwrap();
        // The value bytes are byte-for-byte unchanged at their original offset.
        assert_eq!(
            &bytes[value_offset as usize..value_offset as usize + 24],
            &original_value_bytes[..]
        );
        let exif = ::exif::Reader::new().read_raw(bytes).unwrap();
        let field = exif
            .get_field(::exif::Tag::GPSLatitude, ::exif::In::PRIMARY)
            .unwrap();
        assert_eq!(format!("{}", field.display_value()), "10 deg 20 min 30 sec");
    }

    #[test]
    fn color_space_srgb_sets_known_value() {
        let mut b = b"II\x2a\0\x08\0\0\0".to_vec();
        push_u16(&mut b, 1, true);
        push_u16(&mut b, 0x8769, true);
        push_u16(&mut b, 4, true);
        push_u32(&mut b, 1, true);
        let exif_ifd_offset_pos = b.len();
        push_u32(&mut b, 0, true);
        push_u32(&mut b, 0, true);

        let exif_ifd_offset = b.len() as u32;
        b[exif_ifd_offset_pos..exif_ifd_offset_pos + 4]
            .copy_from_slice(&exif_ifd_offset.to_le_bytes());
        push_u16(&mut b, 1, true);
        push_u16(&mut b, 0xa001, true); // ColorSpace
        push_u16(&mut b, 3, true); // SHORT
        push_u32(&mut b, 1, true);
        push_u32(&mut b, 0xFFFF, true); // Uncalibrated
        push_u32(&mut b, 0, true);

        let facts = OutputFacts {
            width: 10,
            height: 10,
            orientation: OrientationAction::Preserve,
            color: ColorAction::Srgb,
        };
        let (bytes, issues) = correct(&b, &facts, &MetadataLimits::default());
        assert!(issues.is_empty(), "{issues:?}");
        let exif = ::exif::Reader::new().read_raw(bytes.unwrap()).unwrap();
        assert_eq!(
            exif.get_field(::exif::Tag::ColorSpace, ::exif::In::PRIMARY)
                .unwrap()
                .value
                .get_uint(0),
            Some(1)
        );
    }

    #[test]
    fn color_space_unverified_removes_declaration_with_issue() {
        let mut b = b"II\x2a\0\x08\0\0\0".to_vec();
        push_u16(&mut b, 1, true);
        push_u16(&mut b, 0x8769, true);
        push_u16(&mut b, 4, true);
        push_u32(&mut b, 1, true);
        let exif_ifd_offset_pos = b.len();
        push_u32(&mut b, 0, true);
        push_u32(&mut b, 0, true);

        let exif_ifd_offset = b.len() as u32;
        b[exif_ifd_offset_pos..exif_ifd_offset_pos + 4]
            .copy_from_slice(&exif_ifd_offset.to_le_bytes());
        push_u16(&mut b, 1, true);
        push_u16(&mut b, 0xa001, true);
        push_u16(&mut b, 3, true);
        push_u32(&mut b, 1, true);
        push_u32(&mut b, 1, true);
        push_u32(&mut b, 0, true);

        let facts = OutputFacts {
            width: 10,
            height: 10,
            orientation: OrientationAction::Preserve,
            color: ColorAction::Unverified,
        };
        let (bytes, issues) = correct(&b, &facts, &MetadataLimits::default());
        let bytes = bytes.expect("structurally valid");
        assert!(issues.iter().any(|i| {
            i.category == MetadataCategory::Exif
                && i.reason == MetadataIssueReason::Unverified
                && i.field.as_deref() == Some("color_space")
        }));
        let exif = ::exif::Reader::new().read_raw(bytes).unwrap();
        assert!(exif
            .get_field(::exif::Tag::ColorSpace, ::exif::In::PRIMARY)
            .is_none());
    }

    #[test]
    fn value_offset_stability_for_retained_scalar() {
        // A retained ASCII tag (Copyright, out-of-line) must sit at exactly
        // its original file offset in the output.
        let mut b = b"II\x2a\0\x08\0\0\0".to_vec();
        push_u16(&mut b, 1, true);
        push_u16(&mut b, 0x8298, true); // Copyright
        push_u16(&mut b, 2, true); // ASCII
        push_u32(&mut b, 6, true); // "abcde\0"
        let value_offset_pos = b.len();
        push_u32(&mut b, 0, true);
        push_u32(&mut b, 0, true);

        let value_offset = b.len() as u32;
        b[value_offset_pos..value_offset_pos + 4].copy_from_slice(&value_offset.to_le_bytes());
        b.extend(b"abcde\0");

        let facts = default_facts(OrientationAction::Preserve, ColorAction::Unchanged);
        let (bytes, issues) = correct(&b, &facts, &MetadataLimits::default());
        assert!(issues.is_empty(), "{issues:?}");
        let bytes = bytes.unwrap();
        assert_eq!(
            &bytes[value_offset as usize..value_offset as usize + 6],
            b"abcde\0"
        );
    }
}
