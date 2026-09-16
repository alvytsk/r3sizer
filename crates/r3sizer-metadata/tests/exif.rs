//! Black-box EXIF correction tests: exercise only the crate's public
//! `correct` entry point (re-exported at the crate root; the `exif` module
//! itself stays private so this file's `use r3sizer_metadata::*` never
//! collides with the independent `exif` (kamadak-exif) crate used below to
//! verify corrected values). The exhaustive tag-by-tag decision matrix
//! (all eight orientations, SHORT promotion, colorspace policy, cycles,
//! overlaps, malformed counts, ...) lives in `src/exif/mod.rs`'s
//! module-local tests, which can see the private `correct` before it was
//! re-exported and stay colocated with the types they exercise; this file
//! is the independent-reader confirmation of the same public contract.

use r3sizer_metadata::*;

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
    let parsed = exif::Reader::new().read_raw(bytes.unwrap()).unwrap();
    assert_eq!(
        parsed
            .get_field(exif::Tag::ImageWidth, exif::In::PRIMARY)
            .unwrap()
            .value
            .get_uint(0),
        Some(100)
    );
    assert_eq!(
        parsed
            .get_field(exif::Tag::ImageLength, exif::In::PRIMARY)
            .unwrap()
            .value
            .get_uint(0),
        Some(50)
    );
    assert_eq!(
        parsed
            .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
            .unwrap()
            .value
            .get_uint(0),
        Some(1)
    );
}

#[test]
fn preserve_keeps_original_orientation() {
    let facts = OutputFacts {
        width: 100,
        height: 50,
        orientation: OrientationAction::Preserve,
        color: ColorAction::Unchanged,
    };
    let (bytes, issues) = correct(&tiny_exif(), &facts, &MetadataLimits::default());
    assert!(issues.is_empty());
    let parsed = exif::Reader::new().read_raw(bytes.unwrap()).unwrap();
    assert_eq!(
        parsed
            .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
            .unwrap()
            .value
            .get_uint(0),
        Some(6)
    );
}

#[test]
fn malformed_header_is_omitted() {
    let facts = OutputFacts {
        width: 10,
        height: 10,
        orientation: OrientationAction::Preserve,
        color: ColorAction::Unchanged,
    };
    let (bytes, issues) = correct(
        b"not a tiff header at all",
        &facts,
        &MetadataLimits::default(),
    );
    assert!(bytes.is_none());
    assert!(issues.iter().any(|i| {
        i.category == MetadataCategory::Exif && i.reason == MetadataIssueReason::Malformed
    }));
}

#[test]
fn bigtiff_is_reported_unsupported() {
    // Magic 43 (BigTIFF) instead of the classic-TIFF 42.
    let raw = b"II\x2b\0\x08\0\0\0\0\0\0\0\0\0\0\0".to_vec();
    let facts = OutputFacts {
        width: 10,
        height: 10,
        orientation: OrientationAction::Preserve,
        color: ColorAction::Unchanged,
    };
    let (bytes, issues) = correct(&raw, &facts, &MetadataLimits::default());
    assert!(bytes.is_none());
    assert!(issues
        .iter()
        .any(|i| i.reason == MetadataIssueReason::Unsupported));
}
