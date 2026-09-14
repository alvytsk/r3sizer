//! Tag-by-tag correction policy and byte-level patching.
//!
//! `apply()` turns a validated `reader::Parsed` tree into corrected TIFF
//! bytes. It decides, per entry, whether to keep it (verbatim or rewritten
//! in place), or drop it -- then checks that nothing it's about to zero
//! aliases anything it's keeping, and only then mutates a byte buffer.
//!
//! Key invariant: every rewrite happens in the entry's own original 4-byte
//! value field. `0x0100`/`0x0101`/`0xa002`/`0xa003` are count-one
//! SHORT/LONG fields, and a LONG value is always exactly 4 bytes -- so
//! promoting SHORT (2 bytes) to LONG (4 bytes) still fits the 4-byte value
//! field with no relocation, ever. No out-of-line value is ever moved;
//! retained out-of-line entries carry their original 4-byte offset field
//! through unchanged.

use std::ops::Range;

use crate::exif::reader::{Endian, Entry};
use crate::exif::reader::{
    EntryValue, Ifd, IfdKind, Parsed, TAG_COLOR_SPACE, TAG_EXIF_IFD, TAG_GPS_IFD, TAG_IMAGE_HEIGHT,
    TAG_IMAGE_WIDTH, TAG_INTEROP_IFD, TAG_INTEROP_INDEX, TAG_JPEG_IF_LENGTH, TAG_JPEG_IF_OFFSET,
    TAG_MAKER_NOTE, TAG_ORIENTATION, TAG_PIXEL_X_DIM, TAG_PIXEL_Y_DIM, TAG_SUB_IFDS, TYPE_ASCII,
    TYPE_LONG, TYPE_SHORT, TYPE_UNDEFINED,
};
use crate::limits::checked_range;
use crate::types::{
    ColorAction, MetadataCategory, MetadataIssue, MetadataIssueReason, OrientationAction,
    OutputFacts,
};

/// EXIF/TIFF entries whose declared type is UNDEFINED but whose contents
/// are a well-known, fixed, offset-free standard field (version codes, the
/// character-code-prefixed comment text of UserComment). Everything else
/// typed UNDEFINED is opaque application-defined data that may embed its
/// own offsets and is dropped.
const SAFE_UNDEFINED_TAGS: &[u16] = &[
    0x9000, // ExifVersion
    0x9101, // ComponentsConfiguration
    0x9286, // UserComment
    0xa000, // FlashpixVersion
    0xa300, // FileSource
    0xa301, // SceneType
];

/// Curated "known good" standard tags per IFD, retained silently. This is
/// deliberately not an exhaustive registry of every TIFF/EXIF/GPS tag --
/// just the common descriptive/measurement fields the brief calls out by
/// name (capture date, copyright, GPS, capture settings). Anything with a
/// resolvable, non-opaque value that ISN'T in this list is still retained
/// (a standard scalar/array/string type carries no pointer semantics of
/// its own), but with an `Unverified` issue: the brief mandates this
/// ("standard scalar unknown values may be retained only with an
/// unverified issue"), so an unrecognized standard-typed tag is never
/// silently treated as equivalent to a vetted one.
const KNOWN_IFD0_TAGS: &[u16] = &[
    0x010e, // ImageDescription
    0x010f, // Make
    0x0110, // Model
    0x011a, // XResolution
    0x011b, // YResolution
    0x0128, // ResolutionUnit
    0x0131, // Software
    0x0132, // DateTime
    0x013b, // Artist
    0x0213, // YCbCrPositioning
    0x8298, // Copyright
];

const KNOWN_EXIF_TAGS: &[u16] = &[
    0x8822, // ExposureProgram
    0x8827, // ISOSpeedRatings
    0x829a, // ExposureTime
    0x829d, // FNumber
    0x9003, // DateTimeOriginal
    0x9004, // DateTimeDigitized
    0x9201, // ShutterSpeedValue
    0x9202, // ApertureValue
    0x9204, // ExposureBiasValue
    0x9205, // MaxApertureValue
    0x9207, // MeteringMode
    0x9208, // LightSource
    0x9209, // Flash
    0x920a, // FocalLength
    0xa402, // ExposureMode
    0xa403, // WhiteBalance
    0xa406, // SceneCaptureType
];

const KNOWN_GPS_TAGS: &[u16] = &[
    0x0000, // GPSVersionID
    0x0001, // GPSLatitudeRef
    0x0002, // GPSLatitude
    0x0003, // GPSLongitudeRef
    0x0004, // GPSLongitude
    0x0005, // GPSAltitudeRef
    0x0006, // GPSAltitude
    0x0007, // GPSTimeStamp
    0x001d, // GPSDateStamp
];

fn is_known_standard_tag(kind: IfdKind, tag: u16) -> bool {
    match kind {
        IfdKind::Ifd0 => KNOWN_IFD0_TAGS.contains(&tag),
        IfdKind::ExifIfd => KNOWN_EXIF_TAGS.contains(&tag),
        IfdKind::GpsIfd => KNOWN_GPS_TAGS.contains(&tag),
        // InteropIndex is the only standard InteropIFD tag and it's already
        // handled specially; Thumbnail IFDs never reach generic_decision at
        // all (the whole IFD is discarded wholesale).
        IfdKind::InteropIfd | IfdKind::Thumbnail => false,
    }
}

/// The selector mapping a dimension tag to the output dimension it holds.
pub(super) fn output_dimension(tag: u16, width: u32, height: u32) -> Option<u32> {
    match tag {
        TAG_IMAGE_WIDTH | TAG_PIXEL_X_DIM => Some(width),
        TAG_IMAGE_HEIGHT | TAG_PIXEL_Y_DIM => Some(height),
        _ => None,
    }
}

struct FinalEntry {
    tag: u16,
    kind: u16,
    count: u32,
    value_field: [u8; 4],
}

enum Decision {
    Keep(FinalEntry),
    /// Retained, but not a curated "known good" tag for this IFD: carries
    /// an `Unverified` issue alongside the kept entry.
    KeepWithIssue(FinalEntry, MetadataIssue),
    Drop(Option<MetadataIssue>),
}

fn issue(category: MetadataCategory, reason: MetadataIssueReason, field: &str) -> MetadataIssue {
    MetadataIssue {
        category,
        reason,
        field: Some(field.to_string()),
    }
}

fn tag_field(tag: u16) -> String {
    format!("0x{tag:04x}")
}

fn verbatim(e: &Entry) -> FinalEntry {
    FinalEntry {
        tag: e.tag,
        kind: e.kind,
        count: e.count,
        value_field: e.value_field,
    }
}

/// Any entry not given specific tag-based handling below.
///
/// - `EntryValue::Invalid`: dropped, `Malformed` -- its extent can't be
///   trusted at all.
/// - Typed UNDEFINED and not in `SAFE_UNDEFINED_TAGS`: dropped,
///   `Unverified` -- opaque application-defined data that may itself embed
///   offsets (this is how MakerNote-shaped garbage and other proprietary
///   blobs get caught even if they arrive under an unexpected tag number).
/// - A curated "known good" tag for this IFD (`is_known_standard_tag`) or
///   a safe-listed UNDEFINED tag: retained silently.
/// - Anything else with a resolvable value: retained, but with an
///   `Unverified` issue. A standard scalar/array/string type carries no
///   pointer semantics of its own, so it's still safe to keep unchanged --
///   but it hasn't been vetted as one of the fields this crate specifically
///   understands, so it isn't blessed as equivalent to a curated one.
fn generic_decision(kind: IfdKind, e: &Entry) -> Decision {
    match e.value {
        EntryValue::Invalid => Decision::Drop(Some(issue(
            MetadataCategory::Exif,
            MetadataIssueReason::Malformed,
            &tag_field(e.tag),
        ))),
        _ => {
            let safe_undefined = e.kind == TYPE_UNDEFINED && SAFE_UNDEFINED_TAGS.contains(&e.tag);
            if e.kind == TYPE_UNDEFINED && !safe_undefined {
                Decision::Drop(Some(issue(
                    MetadataCategory::Unknown,
                    MetadataIssueReason::Unverified,
                    &tag_field(e.tag),
                )))
            } else if safe_undefined || is_known_standard_tag(kind, e.tag) {
                Decision::Keep(verbatim(e))
            } else {
                Decision::KeepWithIssue(
                    verbatim(e),
                    issue(
                        MetadataCategory::Exif,
                        MetadataIssueReason::Unverified,
                        &tag_field(e.tag),
                    ),
                )
            }
        }
    }
}

fn dimension_decision(e: &Entry, endian: Endian, new_value: u32) -> Decision {
    let valid_shape = e.count == 1
        && matches!(e.value, EntryValue::Inline)
        && (e.kind == TYPE_SHORT || e.kind == TYPE_LONG);
    if !valid_shape {
        return Decision::Drop(Some(issue(
            MetadataCategory::Exif,
            MetadataIssueReason::Malformed,
            &tag_field(e.tag),
        )));
    }
    if e.kind == TYPE_SHORT && new_value <= u16::MAX as u32 {
        let mut value_field = [0u8; 4];
        value_field[0..2].copy_from_slice(&endian.write_u16(new_value as u16));
        Decision::Keep(FinalEntry {
            tag: e.tag,
            kind: TYPE_SHORT,
            count: 1,
            value_field,
        })
    } else {
        // Promote to LONG when the SHORT can no longer hold the value, or
        // keep LONG as LONG. Either way this is still exactly 4 bytes,
        // still the entry's own inline value field: no relocation.
        Decision::Keep(FinalEntry {
            tag: e.tag,
            kind: TYPE_LONG,
            count: 1,
            value_field: endian.write_u32(new_value),
        })
    }
}

fn orientation_decision(e: &Entry, endian: Endian, action: OrientationAction) -> Decision {
    let valid_shape = e.count == 1 && e.kind == TYPE_SHORT && matches!(e.value, EntryValue::Inline);
    if !valid_shape {
        return Decision::Drop(Some(issue(
            MetadataCategory::Exif,
            MetadataIssueReason::Malformed,
            "orientation",
        )));
    }
    let current = endian.u16([e.value_field[0], e.value_field[1]]);
    if !(1..=8).contains(&current) {
        return Decision::Drop(Some(issue(
            MetadataCategory::Exif,
            MetadataIssueReason::Malformed,
            "orientation",
        )));
    }
    match action {
        OrientationAction::Preserve => Decision::Keep(verbatim(e)),
        OrientationAction::Normalize => {
            let mut value_field = [0u8; 4];
            value_field[0..2].copy_from_slice(&endian.write_u16(1));
            Decision::Keep(FinalEntry {
                tag: e.tag,
                kind: e.kind,
                count: e.count,
                value_field,
            })
        }
    }
}

fn colorspace_decision(e: &Entry, endian: Endian, color: ColorAction) -> Decision {
    match color {
        ColorAction::Unchanged => Decision::Keep(verbatim(e)),
        ColorAction::Srgb => {
            let valid_shape =
                e.count == 1 && e.kind == TYPE_SHORT && matches!(e.value, EntryValue::Inline);
            if !valid_shape {
                return Decision::Drop(Some(issue(
                    MetadataCategory::Exif,
                    MetadataIssueReason::Malformed,
                    "color_space",
                )));
            }
            let mut value_field = [0u8; 4];
            value_field[0..2].copy_from_slice(&endian.write_u16(1)); // 1 = sRGB
            Decision::Keep(FinalEntry {
                tag: e.tag,
                kind: e.kind,
                count: e.count,
                value_field,
            })
        }
        ColorAction::Unverified => Decision::Drop(Some(issue(
            MetadataCategory::Exif,
            MetadataIssueReason::Unverified,
            "color_space",
        ))),
    }
}

fn interop_index_decision(e: &Entry, color: ColorAction) -> Decision {
    match color {
        ColorAction::Unchanged => Decision::Keep(verbatim(e)),
        ColorAction::Srgb => {
            let valid_shape =
                e.count == 4 && e.kind == TYPE_ASCII && matches!(e.value, EntryValue::Inline);
            if !valid_shape {
                return Decision::Drop(Some(issue(
                    MetadataCategory::Exif,
                    MetadataIssueReason::Malformed,
                    "interop_index",
                )));
            }
            Decision::Keep(FinalEntry {
                tag: e.tag,
                kind: e.kind,
                count: e.count,
                value_field: *b"R98\0",
            })
        }
        ColorAction::Unverified => Decision::Drop(Some(issue(
            MetadataCategory::Exif,
            MetadataIssueReason::Unverified,
            "interop_index",
        ))),
    }
}

fn decide(kind: IfdKind, e: &Entry, endian: Endian, facts: &OutputFacts) -> Decision {
    if matches!(kind, IfdKind::Ifd0 | IfdKind::ExifIfd) {
        if let Some(new_value) = output_dimension(e.tag, facts.width, facts.height) {
            return dimension_decision(e, endian, new_value);
        }
    }
    match (kind, e.tag) {
        (IfdKind::Ifd0, TAG_ORIENTATION) => orientation_decision(e, endian, facts.orientation),
        (IfdKind::Ifd0, TAG_EXIF_IFD) | (IfdKind::Ifd0, TAG_GPS_IFD) => Decision::Keep(verbatim(e)),
        (IfdKind::ExifIfd, TAG_COLOR_SPACE) => colorspace_decision(e, endian, facts.color),
        (IfdKind::ExifIfd, TAG_INTEROP_IFD) => Decision::Keep(verbatim(e)),
        (IfdKind::InteropIfd, TAG_INTEROP_INDEX) => interop_index_decision(e, facts.color),
        (IfdKind::Ifd0, TAG_MAKER_NOTE) | (IfdKind::ExifIfd, TAG_MAKER_NOTE) => {
            Decision::Drop(Some(issue(
                MetadataCategory::MakerNote,
                MetadataIssueReason::Unverified,
                "maker_note",
            )))
        }
        (IfdKind::Ifd0, TAG_SUB_IFDS) | (IfdKind::ExifIfd, TAG_SUB_IFDS) => {
            Decision::Drop(Some(issue(
                MetadataCategory::Unknown,
                MetadataIssueReason::Unverified,
                "sub_ifds",
            )))
        }
        (IfdKind::Ifd0, TAG_JPEG_IF_OFFSET)
        | (IfdKind::Ifd0, TAG_JPEG_IF_LENGTH)
        | (IfdKind::ExifIfd, TAG_JPEG_IF_OFFSET)
        | (IfdKind::ExifIfd, TAG_JPEG_IF_LENGTH) => Decision::Drop(Some(issue(
            MetadataCategory::Thumbnail,
            MetadataIssueReason::RemovedStale,
            "preview",
        ))),
        _ => generic_decision(kind, e),
    }
}

/// Result of planning one IFD's entries: the rebuilt entry list, the
/// original out-of-line ranges that must survive untouched (paired with
/// each kept entry that has one), the ranges that must be zeroed because
/// their owning entry was dropped, and the issues raised along the way.
struct Plan {
    kept: Vec<FinalEntry>,
    retained_ranges: Vec<Range<usize>>,
    zero_ranges: Vec<Range<usize>>,
    issues: Vec<MetadataIssue>,
}

fn plan_ifd(ifd: &Ifd, endian: Endian, facts: &OutputFacts, raw_len: usize) -> Plan {
    let mut kept = Vec::new();
    let mut retained_ranges = Vec::new();
    let mut zero_ranges = Vec::new();
    let mut issues = Vec::new();

    for e in &ifd.entries {
        match decide(ifd.kind, e, endian, facts) {
            Decision::Keep(final_entry) => {
                if let EntryValue::OutOfLine(r) = &e.value {
                    retained_ranges.push(r.clone());
                }
                kept.push(final_entry);
            }
            Decision::KeepWithIssue(final_entry, iss) => {
                if let EntryValue::OutOfLine(r) = &e.value {
                    retained_ranges.push(r.clone());
                }
                kept.push(final_entry);
                issues.push(iss);
            }
            Decision::Drop(issue) => {
                if let EntryValue::OutOfLine(r) = &e.value {
                    zero_ranges.push(r.clone());
                }
                if let Some(i) = issue {
                    issues.push(i);
                }
            }
        }
    }

    // JPEGInterchangeFormat/Length (0x0201/0x0202) are always inline scalars
    // (count-one LONGs), so the reader has no way to know their *numeric
    // values* are themselves an offset+length pointing at real preview
    // bytes elsewhere in the file. `decide()` above only drops the two
    // directory entries; this resolves what they pointed at so the actual
    // preview bytes get zeroed too, not just the pointer.
    if let Some(r) = resolve_preview_range(&ifd.entries, endian, raw_len) {
        zero_ranges.push(r);
    }

    Plan {
        kept,
        retained_ranges,
        zero_ranges,
        issues,
    }
}

/// Resolve a JPEGInterchangeFormat (`0x0201`) / JPEGInterchangeFormatLength
/// (`0x0202`) pair, if both are present with a valid count-one LONG shape,
/// into the byte range of the preview JPEG they point at. Returns `None`
/// if either tag is missing/malformed or the resolved range doesn't fit in
/// the buffer -- callers already treat "nothing to zero" as safe.
fn resolve_preview_range(
    entries: &[Entry],
    endian: Endian,
    raw_len: usize,
) -> Option<Range<usize>> {
    let offset_entry = entries.iter().find(|e| e.tag == TAG_JPEG_IF_OFFSET)?;
    let length_entry = entries.iter().find(|e| e.tag == TAG_JPEG_IF_LENGTH)?;
    let valid =
        |e: &Entry| e.kind == TYPE_LONG && e.count == 1 && matches!(e.value, EntryValue::Inline);
    if !valid(offset_entry) || !valid(length_entry) {
        return None;
    }
    let offset = endian.u32(offset_entry.value_field) as usize;
    let length = endian.u32(length_entry.value_field) as usize;
    checked_range(offset, length, 1, raw_len)
}

fn rebuild_table(
    endian: Endian,
    span_len: usize,
    entries: &[FinalEntry],
    next_ifd_value: [u8; 4],
) -> Vec<u8> {
    let mut buf = vec![0u8; span_len];
    buf[0..2].copy_from_slice(&endian.write_u16(entries.len() as u16));
    for (i, fe) in entries.iter().enumerate() {
        let pos = 2 + i * 12;
        buf[pos..pos + 2].copy_from_slice(&endian.write_u16(fe.tag));
        buf[pos + 2..pos + 4].copy_from_slice(&endian.write_u16(fe.kind));
        buf[pos + 4..pos + 8].copy_from_slice(&endian.write_u32(fe.count));
        buf[pos + 8..pos + 12].copy_from_slice(&fe.value_field);
    }
    let next_pos = 2 + entries.len() * 12;
    buf[next_pos..next_pos + 4].copy_from_slice(&next_ifd_value);
    buf
}

/// Sweep-line check: `spans` mixes ranges that must remain exclusively
/// owned by what's being kept (`retained = true`: a kept IFD's table span,
/// or a kept entry's out-of-line value) with ranges about to be
/// overwritten/zeroed (`retained = false`: a discarded thumbnail-chain IFD
/// or a dropped entry's value). Returns `true` if any retained range
/// overlaps any other range -- two discarded ranges overlapping each other
/// is fine (both are being destroyed anyway), but a retained range must be
/// exclusively its own.
fn has_unsafe_overlap(mut spans: Vec<(Range<usize>, bool)>) -> bool {
    spans.sort_by_key(|(r, _)| r.start);
    let mut cluster_end = 0usize;
    let mut cluster_has_retained = false;
    let mut cluster_len = 0usize;
    let mut unsafe_found = false;
    for (r, retained) in spans {
        if cluster_len > 0 && r.start < cluster_end {
            cluster_len += 1;
            cluster_end = cluster_end.max(r.end);
            cluster_has_retained = cluster_has_retained || retained;
        } else {
            cluster_end = r.end;
            cluster_has_retained = retained;
            cluster_len = 1;
        }
        unsafe_found = unsafe_found || (cluster_len > 1 && cluster_has_retained);
    }
    unsafe_found
}

struct KeptIfd<'a> {
    ifd: &'a Ifd,
    plan: Plan,
    next_ifd_value: [u8; 4],
}

pub(super) fn apply(
    raw: &[u8],
    parsed: &Parsed,
    facts: &OutputFacts,
) -> Result<(Vec<u8>, Vec<MetadataIssue>), MetadataIssue> {
    let endian = parsed.endian;
    let mut all_issues = Vec::new();
    let mut bucket_a: Vec<(Range<usize>, bool)> = Vec::new();
    let mut bucket_b: Vec<(Range<usize>, bool)> = Vec::new();

    let mut kept_ifds: Vec<KeptIfd> = Vec::new();

    let ifd0_plan = plan_ifd(&parsed.ifd0, endian, facts, raw.len());
    push_plan(&mut bucket_a, &mut bucket_b, &parsed.ifd0, &ifd0_plan);
    all_issues.extend(ifd0_plan.issues.iter().cloned());
    kept_ifds.push(KeptIfd {
        ifd: &parsed.ifd0,
        plan: ifd0_plan,
        next_ifd_value: [0, 0, 0, 0], // thumbnail chain is always detached
    });

    for ifd in [&parsed.exif_ifd, &parsed.gps_ifd, &parsed.interop_ifd]
        .into_iter()
        .flatten()
    {
        let plan = plan_ifd(ifd, endian, facts, raw.len());
        push_plan(&mut bucket_a, &mut bucket_b, ifd, &plan);
        all_issues.extend(plan.issues.iter().cloned());
        let mut next_ifd_value = [0u8; 4];
        next_ifd_value.copy_from_slice(&raw[ifd.next_ifd_field_pos..ifd.next_ifd_field_pos + 4]);
        kept_ifds.push(KeptIfd {
            ifd,
            plan,
            next_ifd_value,
        });
    }

    if !parsed.thumbnail_chain.is_empty() {
        for ifd in &parsed.thumbnail_chain {
            bucket_b.push((ifd.table_span.clone(), false));
            for e in &ifd.entries {
                if let EntryValue::OutOfLine(r) = &e.value {
                    bucket_b.push((r.clone(), false));
                }
            }
            // The thumbnail IFD's own JPEGInterchangeFormat/Length pair (if
            // present) points at the actual thumbnail JPEG bytes elsewhere
            // in the file -- resolve and zero those too, not just the IFD's
            // table and its `OutOfLine` entries.
            if let Some(r) = resolve_preview_range(&ifd.entries, endian, raw.len()) {
                bucket_b.push((r, false));
            }
        }
        all_issues.push(issue(
            MetadataCategory::Thumbnail,
            MetadataIssueReason::RemovedStale,
            "thumbnail_chain",
        ));
    }

    let mut all_spans = bucket_a;
    all_spans.extend(bucket_b);
    if has_unsafe_overlap(all_spans) {
        return Err(issue(
            MetadataCategory::Exif,
            MetadataIssueReason::Malformed,
            "aliasing",
        ));
    }

    let mut out = raw.to_vec();
    for kept in &kept_ifds {
        let span_len = kept.ifd.table_span.len();
        let table = rebuild_table(endian, span_len, &kept.plan.kept, kept.next_ifd_value);
        out[kept.ifd.table_span.clone()].copy_from_slice(&table);
    }
    for ifd in &parsed.thumbnail_chain {
        out[ifd.table_span.clone()].fill(0);
        for e in &ifd.entries {
            if let EntryValue::OutOfLine(r) = &e.value {
                out[r.clone()].fill(0);
            }
        }
        if let Some(r) = resolve_preview_range(&ifd.entries, endian, raw.len()) {
            out[r].fill(0);
        }
    }
    for kept in &kept_ifds {
        for r in &kept.plan.zero_ranges {
            out[r.clone()].fill(0);
        }
    }

    Ok((out, all_issues))
}

fn push_plan(
    bucket_a: &mut Vec<(Range<usize>, bool)>,
    bucket_b: &mut Vec<(Range<usize>, bool)>,
    ifd: &Ifd,
    plan: &Plan,
) {
    bucket_a.push((ifd.table_span.clone(), true));
    for r in &plan.retained_ranges {
        bucket_a.push((r.clone(), true));
    }
    for r in &plan.zero_ranges {
        bucket_b.push((r.clone(), false));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_dimension_selector() {
        assert_eq!(output_dimension(0x0100, 800, 600), Some(800));
        assert_eq!(output_dimension(0x0101, 800, 600), Some(600));
        assert_eq!(output_dimension(0xa002, 800, 600), Some(800));
        assert_eq!(output_dimension(0xa003, 800, 600), Some(600));
        assert_eq!(output_dimension(0x0112, 800, 600), None);
        assert_eq!(output_dimension(0x829a, 800, 600), None);
    }
}
