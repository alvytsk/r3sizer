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

/// Any entry not given specific tag-based handling below. A value typed
/// UNDEFINED is treated as an opaque, potentially offset-bearing blob unless
/// its tag is a known-safe standard field; anything else with a resolvable
/// value is retained as-is (a standard scalar/array/string type carries no
/// pointer semantics of its own, so its offset, if any, is safe to keep
/// unchanged).
fn generic_decision(e: &Entry) -> Decision {
    match e.value {
        EntryValue::Invalid => Decision::Drop(Some(issue(
            MetadataCategory::Exif,
            MetadataIssueReason::Malformed,
            &tag_field(e.tag),
        ))),
        _ => {
            if e.kind == TYPE_UNDEFINED && !SAFE_UNDEFINED_TAGS.contains(&e.tag) {
                Decision::Drop(Some(issue(
                    MetadataCategory::Unknown,
                    MetadataIssueReason::Unverified,
                    &tag_field(e.tag),
                )))
            } else {
                Decision::Keep(verbatim(e))
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
        _ => generic_decision(e),
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

fn plan_ifd(ifd: &Ifd, endian: Endian, facts: &OutputFacts) -> Plan {
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

    Plan {
        kept,
        retained_ranges,
        zero_ranges,
        issues,
    }
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

    let ifd0_plan = plan_ifd(&parsed.ifd0, endian, facts);
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
        let plan = plan_ifd(ifd, endian, facts);
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
