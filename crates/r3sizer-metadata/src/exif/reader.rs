//! Bounded TIFF/EXIF structural parser.
//!
//! Walks a classic (32-bit) TIFF byte-order-marked buffer -- the format
//! Task 2 hands us for an EXIF payload (starting at `II`/`MM`, no leading
//! `Exif\0\0`) -- and resolves IFD0, the standard sub-IFDs reachable from it
//! (ExifIFD via `0x8769`, GPSIFD via `0x8825` in IFD0, InteropIFD via
//! `0xa005` in ExifIFD), and the next-IFD chain hanging off IFD0
//! (conventionally the embedded thumbnail). Every offset is checked against
//! `raw.len()` before use via `checked_range`; nothing here trusts a pointer
//! just because it parses as a number.
//!
//! This module never decides what to keep or drop -- that's `patch.rs`'s
//! job. It only answers "where are the tables, where are the entries, where
//! do their values live, and is the structure sound enough to act on."
//!
//! Design choice: any structural inconsistency that would make it unsafe to
//! locate the *rest* of the structure (a truncated table, a cycle, a
//! duplicate tag, a sub-IFD pointer that doesn't resolve, exceeding
//! `max_ifds`/`max_exif_entries`) fails the whole parse rather than trying
//! to salvage a partial tree. This matches the conservative "omit rather
//! than guess" posture used elsewhere in this crate (see
//! `containers::assemble_icc`, `MetadataBundle::unavailable`). A single
//! entry with an unrecognized *type* doesn't threaten that -- entries are a
//! fixed 12 bytes regardless of their declared type -- so that case is
//! recorded per-entry (`EntryValue::Invalid`) and left for `patch.rs` to
//! drop with an issue, without aborting the rest of the table.
use std::collections::HashSet;
use std::ops::Range;

use crate::limits::{checked_range, MetadataLimits};
use crate::types::{MetadataCategory, MetadataIssue, MetadataIssueReason};

pub(super) const TAG_IMAGE_WIDTH: u16 = 0x0100;
pub(super) const TAG_IMAGE_HEIGHT: u16 = 0x0101;
pub(super) const TAG_ORIENTATION: u16 = 0x0112;
pub(super) const TAG_EXIF_IFD: u16 = 0x8769;
pub(super) const TAG_GPS_IFD: u16 = 0x8825;
pub(super) const TAG_INTEROP_IFD: u16 = 0xa005;
pub(super) const TAG_PIXEL_X_DIM: u16 = 0xa002;
pub(super) const TAG_PIXEL_Y_DIM: u16 = 0xa003;
pub(super) const TAG_COLOR_SPACE: u16 = 0xa001;
pub(super) const TAG_INTEROP_INDEX: u16 = 0x0001;
pub(super) const TAG_MAKER_NOTE: u16 = 0x927c;
pub(super) const TAG_SUB_IFDS: u16 = 0x014a;
pub(super) const TAG_JPEG_IF_OFFSET: u16 = 0x0201;
pub(super) const TAG_JPEG_IF_LENGTH: u16 = 0x0202;

pub(super) const TYPE_ASCII: u16 = 2;
pub(super) const TYPE_SHORT: u16 = 3;
pub(super) const TYPE_LONG: u16 = 4;
pub(super) const TYPE_UNDEFINED: u16 = 7;

/// TIFF byte order. Only classic (magic 42) little/big endian is supported;
/// BigTIFF (magic 43) is rejected as `Unsupported` before this is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Endian {
    Little,
    Big,
}

impl Endian {
    pub(super) fn u16(self, b: [u8; 2]) -> u16 {
        match self {
            Endian::Little => u16::from_le_bytes(b),
            Endian::Big => u16::from_be_bytes(b),
        }
    }

    pub(super) fn u32(self, b: [u8; 4]) -> u32 {
        match self {
            Endian::Little => u32::from_le_bytes(b),
            Endian::Big => u32::from_be_bytes(b),
        }
    }

    pub(super) fn write_u16(self, v: u16) -> [u8; 2] {
        match self {
            Endian::Little => v.to_le_bytes(),
            Endian::Big => v.to_be_bytes(),
        }
    }

    pub(super) fn write_u32(self, v: u32) -> [u8; 4] {
        match self {
            Endian::Little => v.to_le_bytes(),
            Endian::Big => v.to_be_bytes(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IfdKind {
    Ifd0,
    ExifIfd,
    GpsIfd,
    InteropIfd,
    Thumbnail,
}

/// Where a directory entry's value lives.
#[derive(Debug, Clone)]
pub(super) enum EntryValue {
    /// `count * width(kind) <= 4`: the value is the entry's own 4-byte
    /// value/offset field, left-justified.
    Inline,
    /// `count * width(kind) > 4`: the value/offset field holds a checked,
    /// in-bounds offset to this range in `raw`.
    OutOfLine(Range<usize>),
    /// The declared type is not one of the twelve baseline TIFF 6.0 types,
    /// or `count * width` overflowed / the resolved offset+length ran off
    /// the end of the buffer. The entry's 12-byte slot is still real (and
    /// still occupies table space, counted against limits), but its value
    /// extent can't be trusted, so it's never read or retained.
    Invalid,
}

/// One parsed 12-byte directory entry.
#[derive(Debug, Clone)]
pub(super) struct Entry {
    pub(super) tag: u16,
    pub(super) kind: u16,
    pub(super) count: u32,
    /// Original 4 raw bytes of the value/offset field, copied verbatim so a
    /// retained entry can be replayed byte-for-byte without re-deriving it.
    pub(super) value_field: [u8; 4],
    pub(super) value: EntryValue,
}

/// One parsed IFD table: `count(2) + entries(12*count) + next_ifd(4)`.
pub(super) struct Ifd {
    pub(super) kind: IfdKind,
    /// Full original byte span of the table, header through next-pointer.
    pub(super) table_span: Range<usize>,
    pub(super) entries: Vec<Entry>,
    /// Position of the 4-byte next-IFD pointer field within `raw`.
    pub(super) next_ifd_field_pos: usize,
    pub(super) next_ifd_offset: u32,
}

pub(super) struct Parsed {
    pub(super) endian: Endian,
    pub(super) ifd0: Ifd,
    pub(super) exif_ifd: Option<Ifd>,
    pub(super) gps_ifd: Option<Ifd>,
    pub(super) interop_ifd: Option<Ifd>,
    /// IFD0's next-IFD chain (conventionally the thumbnail IFD and, rarely,
    /// further links). Always discarded wholesale by `patch.rs`.
    pub(super) thumbnail_chain: Vec<Ifd>,
}

pub(super) fn find_entry(ifd: &Ifd, tag: u16) -> Option<&Entry> {
    ifd.entries.iter().find(|e| e.tag == tag)
}

/// Byte width of one value of TIFF baseline type `kind`, or `None` if
/// `kind` isn't one of the twelve TIFF 6.0 types.
fn type_width(kind: u16) -> Option<usize> {
    match kind {
        1 | 2 | 6 | 7 => Some(1), // BYTE, ASCII, SBYTE, UNDEFINED
        3 | 8 => Some(2),         // SHORT, SSHORT
        4 | 9 | 11 => Some(4),    // LONG, SLONG, FLOAT
        5 | 10 | 12 => Some(8),   // RATIONAL, SRATIONAL, DOUBLE
        _ => None,
    }
}

fn fatal(reason: MetadataIssueReason, field: &str) -> MetadataIssue {
    MetadataIssue {
        category: MetadataCategory::Exif,
        reason,
        field: Some(field.to_string()),
    }
}

/// Parse the full IFD tree rooted at the TIFF header in `raw`.
pub(super) fn parse(raw: &[u8], limits: &MetadataLimits) -> Result<Parsed, MetadataIssue> {
    if raw.len() < 8 {
        return Err(fatal(MetadataIssueReason::Malformed, "header"));
    }
    let endian = match &raw[0..2] {
        [b'I', b'I'] => Endian::Little,
        [b'M', b'M'] => Endian::Big,
        _ => return Err(fatal(MetadataIssueReason::Malformed, "byte_order")),
    };
    let magic = endian.u16([raw[2], raw[3]]);
    if magic == 43 {
        return Err(fatal(MetadataIssueReason::Unsupported, "bigtiff"));
    }
    if magic != 42 {
        return Err(fatal(MetadataIssueReason::Malformed, "magic"));
    }
    let ifd0_offset = endian.u32([raw[4], raw[5], raw[6], raw[7]]) as usize;

    let mut visited: HashSet<usize> = HashSet::new();
    let mut ifd_budget = limits.max_ifds;
    let mut entry_budget = limits.max_exif_entries;

    let ifd0 = parse_ifd(
        raw,
        endian,
        ifd0_offset,
        IfdKind::Ifd0,
        &mut visited,
        &mut ifd_budget,
        &mut entry_budget,
    )?;

    let exif_ifd = match find_entry(&ifd0, TAG_EXIF_IFD) {
        Some(e) => {
            let off = resolve_pointer(e, endian)
                .ok_or_else(|| fatal(MetadataIssueReason::Malformed, "exif_ifd_pointer"))?;
            Some(parse_ifd(
                raw,
                endian,
                off,
                IfdKind::ExifIfd,
                &mut visited,
                &mut ifd_budget,
                &mut entry_budget,
            )?)
        }
        None => None,
    };

    let gps_ifd = match find_entry(&ifd0, TAG_GPS_IFD) {
        Some(e) => {
            let off = resolve_pointer(e, endian)
                .ok_or_else(|| fatal(MetadataIssueReason::Malformed, "gps_ifd_pointer"))?;
            Some(parse_ifd(
                raw,
                endian,
                off,
                IfdKind::GpsIfd,
                &mut visited,
                &mut ifd_budget,
                &mut entry_budget,
            )?)
        }
        None => None,
    };

    let interop_ifd = match exif_ifd
        .as_ref()
        .and_then(|e| find_entry(e, TAG_INTEROP_IFD))
    {
        Some(e) => {
            let off = resolve_pointer(e, endian)
                .ok_or_else(|| fatal(MetadataIssueReason::Malformed, "interop_ifd_pointer"))?;
            Some(parse_ifd(
                raw,
                endian,
                off,
                IfdKind::InteropIfd,
                &mut visited,
                &mut ifd_budget,
                &mut entry_budget,
            )?)
        }
        None => None,
    };

    let mut thumbnail_chain = Vec::new();
    let mut next = ifd0.next_ifd_offset;
    while next != 0 {
        let ifd = parse_ifd(
            raw,
            endian,
            next as usize,
            IfdKind::Thumbnail,
            &mut visited,
            &mut ifd_budget,
            &mut entry_budget,
        )?;
        next = ifd.next_ifd_offset;
        thumbnail_chain.push(ifd);
    }

    Ok(Parsed {
        endian,
        ifd0,
        exif_ifd,
        gps_ifd,
        interop_ifd,
        thumbnail_chain,
    })
}

/// A sub-IFD pointer tag (`ExifIFD`/`GPSIFD`/`InteropIFD`) is always a
/// count-one LONG, which is always inline (4 bytes fits the 4-byte value
/// field). Anything else means the pointer itself is malformed.
fn resolve_pointer(entry: &Entry, endian: Endian) -> Option<usize> {
    if entry.kind != TYPE_LONG || entry.count != 1 {
        return None;
    }
    match entry.value {
        EntryValue::Inline => Some(endian.u32(entry.value_field) as usize),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn parse_ifd(
    raw: &[u8],
    endian: Endian,
    offset: usize,
    kind: IfdKind,
    visited: &mut HashSet<usize>,
    ifd_budget: &mut usize,
    entry_budget: &mut usize,
) -> Result<Ifd, MetadataIssue> {
    if !visited.insert(offset) {
        return Err(fatal(MetadataIssueReason::Malformed, "cycle"));
    }
    if *ifd_budget == 0 {
        return Err(fatal(MetadataIssueReason::LimitExceeded, "max_ifds"));
    }
    *ifd_budget -= 1;

    let count_range = checked_range(offset, 2, 1, raw.len())
        .ok_or_else(|| fatal(MetadataIssueReason::Malformed, "ifd_header"))?;
    let count = endian.u16([raw[count_range.start], raw[count_range.start + 1]]);

    if count as usize > *entry_budget {
        return Err(fatal(
            MetadataIssueReason::LimitExceeded,
            "max_exif_entries",
        ));
    }
    *entry_budget -= count as usize;

    let entries_span = checked_range(count_range.end, count as usize, 12, raw.len())
        .ok_or_else(|| fatal(MetadataIssueReason::Malformed, "ifd_entries"))?;
    let next_field = checked_range(entries_span.end, 4, 1, raw.len())
        .ok_or_else(|| fatal(MetadataIssueReason::Malformed, "ifd_next"))?;

    let mut entries = Vec::with_capacity(count as usize);
    let mut seen_tags: HashSet<u16> = HashSet::with_capacity(count as usize);
    for i in 0..count as usize {
        let pos = entries_span.start + i * 12;
        let tag = endian.u16([raw[pos], raw[pos + 1]]);
        if !seen_tags.insert(tag) {
            return Err(fatal(MetadataIssueReason::Malformed, "duplicate_tag"));
        }
        let ekind = endian.u16([raw[pos + 2], raw[pos + 3]]);
        let ecount = endian.u32([raw[pos + 4], raw[pos + 5], raw[pos + 6], raw[pos + 7]]);
        let value_field_pos = pos + 8;
        let mut value_field = [0u8; 4];
        value_field.copy_from_slice(&raw[value_field_pos..value_field_pos + 4]);

        let value = resolve_value(endian, ekind, ecount, value_field, raw);
        entries.push(Entry {
            tag,
            kind: ekind,
            count: ecount,
            value_field,
            value,
        });
    }

    let next_ifd_offset = endian.u32([
        raw[next_field.start],
        raw[next_field.start + 1],
        raw[next_field.start + 2],
        raw[next_field.start + 3],
    ]);

    Ok(Ifd {
        kind,
        table_span: offset..next_field.end,
        entries,
        next_ifd_field_pos: next_field.start,
        next_ifd_offset,
    })
}

fn resolve_value(
    endian: Endian,
    kind: u16,
    count: u32,
    value_field: [u8; 4],
    raw: &[u8],
) -> EntryValue {
    let Some(width) = type_width(kind) else {
        return EntryValue::Invalid;
    };
    let Some(total_len) = (count as usize).checked_mul(width) else {
        return EntryValue::Invalid;
    };
    if total_len <= 4 {
        return EntryValue::Inline;
    }
    let voff = endian.u32(value_field) as usize;
    match checked_range(voff, total_len, 1, raw.len()) {
        Some(r) => EntryValue::OutOfLine(r),
        None => EntryValue::Invalid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_width_covers_baseline_types_only() {
        assert_eq!(type_width(1), Some(1)); // BYTE
        assert_eq!(type_width(2), Some(1)); // ASCII
        assert_eq!(type_width(3), Some(2)); // SHORT
        assert_eq!(type_width(4), Some(4)); // LONG
        assert_eq!(type_width(5), Some(8)); // RATIONAL
        assert_eq!(type_width(12), Some(8)); // DOUBLE
        assert_eq!(type_width(13), None); // IFD (not baseline here)
        assert_eq!(type_width(0), None);
        assert_eq!(type_width(99), None);
    }
}
