//! JPEG (JFIF/Exif) container scanning.
//!
//! Walks marker segments from SOI to EOI without decoding any entropy-coded
//! (pixel) data. Recognizes APP0 JFIF density, APP1 Exif/XMP, APP2 ICC
//! profile sequences, APP13 Photoshop IRBs (delegated to `crate::iptc`),
//! and COM comments. Anything else with a length field is skipped by
//! length; entropy-coded scan data is skipped using the stuffed-byte /
//! restart-marker rules so metadata placed after later scans (progressive
//! JPEGs) is still found.

use std::collections::BTreeMap;

use img_parts::jpeg::{markers, Jpeg, JpegSegment};
use img_parts::{Bytes, ImageEXIF, ImageICC};

use crate::bundle::{MetadataBundle, Payload, SourceColor, SourceFormat};
use crate::containers::Collector;
use crate::limits::{checked_range, MetadataLimits};
use crate::types::{
    MetadataCategory, MetadataExport, MetadataIssueReason, MetadataReport, OutputFacts,
};

const EXIF_PREFIX: &[u8] = b"Exif\0\0";
const XMP_STANDARD_PREFIX: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const XMP_EXTENDED_PREFIX: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";
const ICC_PREFIX: &[u8] = b"ICC_PROFILE\0";
const PHOTOSHOP_PREFIX: &[u8] = b"Photoshop 3.0\0";

const MARKER_EOI: u8 = 0xD9;
const MARKER_TEM: u8 = 0x01;
const MARKER_SOS: u8 = 0xDA;
const MARKER_COM: u8 = 0xFE;

pub(crate) fn extract(source: &[u8], limits: &MetadataLimits) -> MetadataBundle {
    let mut collector = Collector::new(limits);
    let mut icc_chunks: Vec<(u8, u8, Vec<u8>)> = Vec::new();
    let mut eoi_found = false;
    let mut fatal = false;

    // `source` starts with SOI (0xFFD8); dispatch already verified this.
    let mut pos = 2usize;

    loop {
        if pos >= source.len() {
            break;
        }
        if source[pos] != 0xFF {
            collector.push_issue(
                MetadataCategory::Unknown,
                MetadataIssueReason::Malformed,
                Some("segment_sync".to_string()),
            );
            fatal = true;
            break;
        }
        // Fill bytes: extra 0xFF padding is legal before a marker code.
        while pos < source.len() && source[pos] == 0xFF {
            pos += 1;
        }
        if pos >= source.len() {
            break;
        }
        let marker = source[pos];
        pos += 1;

        match marker {
            MARKER_EOI => {
                eoi_found = true;
                break;
            }
            MARKER_TEM | 0xD0..=0xD7 => {
                // Standalone markers with no length field.
                continue;
            }
            MARKER_SOS => {
                let Some(len) = read_u16(source, pos) else {
                    collector.push_issue(
                        MetadataCategory::Unknown,
                        MetadataIssueReason::Malformed,
                        Some("sos_truncated".to_string()),
                    );
                    fatal = true;
                    break;
                };
                let Some(seg_range) = valid_segment_range(pos, len as usize, source.len()) else {
                    collector.push_issue(
                        MetadataCategory::Unknown,
                        MetadataIssueReason::Malformed,
                        Some("sos_length".to_string()),
                    );
                    fatal = true;
                    break;
                };
                pos = skip_entropy_scan(source, seg_range.end);
                continue;
            }
            0xE0..=0xEF | MARKER_COM => {
                let Some(len) = read_u16(source, pos) else {
                    collector.push_issue(
                        MetadataCategory::Unknown,
                        MetadataIssueReason::Malformed,
                        Some("segment_truncated".to_string()),
                    );
                    fatal = true;
                    break;
                };
                let Some(seg_range) = valid_segment_range(pos, len as usize, source.len()) else {
                    collector.push_issue(
                        MetadataCategory::Unknown,
                        MetadataIssueReason::Malformed,
                        Some("segment_length".to_string()),
                    );
                    fatal = true;
                    break;
                };
                let seg_data = &source[pos + 2..seg_range.end];
                handle_segment(marker, seg_data, &mut collector, &mut icc_chunks);
                pos = seg_range.end;
                continue;
            }
            _ => {
                // SOF*, DQT, DHT, DRI, DNL, etc: pixel/structural segments,
                // skipped by declared length.
                let Some(len) = read_u16(source, pos) else {
                    collector.push_issue(
                        MetadataCategory::Unknown,
                        MetadataIssueReason::Malformed,
                        Some("segment_truncated".to_string()),
                    );
                    fatal = true;
                    break;
                };
                let Some(seg_range) = valid_segment_range(pos, len as usize, source.len()) else {
                    collector.push_issue(
                        MetadataCategory::Unknown,
                        MetadataIssueReason::Malformed,
                        Some("segment_length".to_string()),
                    );
                    fatal = true;
                    break;
                };
                pos = seg_range.end;
                continue;
            }
        }
    }

    if !eoi_found && !fatal {
        collector.push_issue(
            MetadataCategory::Unknown,
            MetadataIssueReason::Malformed,
            Some("missing_eoi".to_string()),
        );
    } else if eoi_found && pos < source.len() {
        collector.push_issue(
            MetadataCategory::Unknown,
            MetadataIssueReason::Unverified,
            Some("trailing".to_string()),
        );
    }

    let icc_added = assemble_icc(icc_chunks, &mut collector);
    let mut bundle = collector.finish(SourceFormat::Jpeg);
    bundle.source_color = if icc_added {
        SourceColor::Other
    } else if fatal {
        SourceColor::Unknown
    } else {
        SourceColor::Unspecified
    };
    bundle
}

fn read_u16(data: &[u8], pos: usize) -> Option<u16> {
    let r = checked_range(pos, 2, 1, data.len())?;
    Some(u16::from_be_bytes([data[r.start], data[r.start + 1]]))
}

/// A marker segment's length field counts itself: total span is
/// `pos..pos+len`, and `len` must be at least 2 (the field itself).
fn valid_segment_range(pos: usize, len: usize, source_len: usize) -> Option<std::ops::Range<usize>> {
    if len < 2 {
        return None;
    }
    checked_range(pos, len, 1, source_len)
}

/// Scan past entropy-coded data starting at `pos`, honoring byte-stuffing
/// (`FF 00` is a literal 0xFF in the data) and restart markers (`FF D0`..
/// `FF D7`, which end a restart interval but not the scan). Returns the
/// position of the next real marker's leading `0xFF`, or `data.len()` if
/// the scan runs off the end (truncated entropy data).
fn skip_entropy_scan(data: &[u8], mut pos: usize) -> usize {
    while pos < data.len() {
        if data[pos] == 0xFF {
            if pos + 1 >= data.len() {
                return pos;
            }
            let next = data[pos + 1];
            match next {
                0x00 => pos += 2,                    // stuffed literal 0xFF
                0xFF => pos += 1,                     // fill byte, re-check
                0xD0..=0xD7 => pos += 2,              // restart marker, scan continues
                _ => return pos,                      // real marker boundary
            }
        } else {
            pos += 1;
        }
    }
    pos
}

fn handle_segment(
    marker: u8,
    data: &[u8],
    collector: &mut Collector,
    icc_chunks: &mut Vec<(u8, u8, Vec<u8>)>,
) {
    match marker {
        0xE0 => handle_app0(data, collector),
        0xE1 => handle_app1(data, collector),
        0xE2 => handle_app2(data, collector, icc_chunks),
        0xED => handle_app13(data, collector),
        MARKER_COM => {
            collector.add_payload(
                Payload::JpegComment(data.to_vec()),
                MetadataCategory::Text,
                "com",
            );
        }
        _ => {
            let n = marker - 0xE0;
            collector.push_issue(
                MetadataCategory::Unknown,
                MetadataIssueReason::Unsupported,
                Some(format!("APP{n}")),
            );
        }
    }
}

fn handle_app0(data: &[u8], collector: &mut Collector) {
    const JFIF_PREFIX: &[u8] = b"JFIF\0";
    if !data.starts_with(JFIF_PREFIX) {
        collector.push_issue(
            MetadataCategory::Unknown,
            MetadataIssueReason::Unsupported,
            Some("APP0".to_string()),
        );
        return;
    }
    // JFIF_PREFIX(5) + version(2) + units(1) + xdensity(2) + ydensity(2) = 12
    if data.len() < 12 {
        collector.push_issue(
            MetadataCategory::Density,
            MetadataIssueReason::Malformed,
            Some("jfif".to_string()),
        );
        return;
    }
    let units = data[7];
    let x = u16::from_be_bytes([data[8], data[9]]);
    let y = u16::from_be_bytes([data[10], data[11]]);
    collector.add_payload(
        Payload::JfifDensity { units, x, y },
        MetadataCategory::Density,
        "jfif",
    );
}

fn handle_app1(data: &[u8], collector: &mut Collector) {
    if let Some(rest) = data.strip_prefix(EXIF_PREFIX) {
        collector.offer_exif(rest.to_vec());
    } else if let Some(rest) = data.strip_prefix(XMP_STANDARD_PREFIX) {
        collector.offer_xmp(rest.to_vec());
    } else if data.starts_with(XMP_EXTENDED_PREFIX) {
        // GUID(32) + full length(4) + offset(4) + chunk data. Reassembling
        // multi-segment extended XMP is XMP-policy work (later task); this
        // task only recognizes the segment so it isn't misfiled as an
        // unsupported APP1.
        collector.push_issue(
            MetadataCategory::Xmp,
            MetadataIssueReason::Unsupported,
            Some("extended".to_string()),
        );
    } else {
        collector.push_issue(
            MetadataCategory::Unknown,
            MetadataIssueReason::Unsupported,
            Some("APP1".to_string()),
        );
    }
}

fn handle_app2(data: &[u8], collector: &mut Collector, icc_chunks: &mut Vec<(u8, u8, Vec<u8>)>) {
    if data.len() >= ICC_PREFIX.len() + 2 && data.starts_with(ICC_PREFIX) {
        let seq_no = data[ICC_PREFIX.len()];
        let count = data[ICC_PREFIX.len() + 1];
        let chunk = data[ICC_PREFIX.len() + 2..].to_vec();
        icc_chunks.push((seq_no, count, chunk));
    } else {
        collector.push_issue(
            MetadataCategory::Unknown,
            MetadataIssueReason::Unsupported,
            Some("APP2".to_string()),
        );
    }
}

fn handle_app13(data: &[u8], collector: &mut Collector) {
    if let Some(rest) = data.strip_prefix(PHOTOSHOP_PREFIX) {
        crate::iptc::parse_irbs(rest, collector);
    } else {
        collector.push_issue(
            MetadataCategory::Unknown,
            MetadataIssueReason::Unsupported,
            Some("APP13".to_string()),
        );
    }
}

/// Assemble APP2 ICC segments collected across the whole file into a single
/// profile. Requires a complete (`1..=count`), unique, and count-consistent
/// sequence; any violation drops the profile and records `Malformed`.
fn assemble_icc(chunks: Vec<(u8, u8, Vec<u8>)>, collector: &mut Collector) -> bool {
    if chunks.is_empty() {
        return false;
    }
    let count = chunks[0].1;
    if count == 0 {
        collector.push_issue(
            MetadataCategory::Icc,
            MetadataIssueReason::Malformed,
            Some("icc_count".to_string()),
        );
        return false;
    }
    if chunks.iter().any(|(_, c, _)| *c != count) {
        collector.push_issue(
            MetadataCategory::Icc,
            MetadataIssueReason::Malformed,
            Some("icc_inconsistent".to_string()),
        );
        return false;
    }
    let mut by_seq: BTreeMap<u8, &Vec<u8>> = BTreeMap::new();
    for (seq, _, data) in &chunks {
        if by_seq.insert(*seq, data).is_some() {
            collector.push_issue(
                MetadataCategory::Icc,
                MetadataIssueReason::Malformed,
                Some("icc_duplicate".to_string()),
            );
            return false;
        }
    }
    if (1..=count).any(|s| !by_seq.contains_key(&s)) {
        collector.push_issue(
            MetadataCategory::Icc,
            MetadataIssueReason::Malformed,
            Some("icc_incomplete".to_string()),
        );
        return false;
    }
    let mut assembled = Vec::new();
    for s in 1..=count {
        assembled.extend_from_slice(by_seq[&s]);
    }
    collector.add_payload(Payload::Icc(assembled), MetadataCategory::Icc, "icc")
}

// --- Merge / embed ---------------------------------------------------------
//
// JPEG APP segments carry a 16-bit big-endian length field that counts
// itself, so the largest legal payload (identifier included) is
// `u16::MAX - 2` bytes. Oversized EXIF/XMP is omitted with an issue, never
// truncated or split (ICC already handles arbitrarily large profiles via
// `set_icc_profile`'s own multi-segment APP2 splitting).
const MAX_APP_PAYLOAD: usize = 65_533;

/// Insert `prepared`'s payloads into an already-encoded destination JPEG.
///
/// Individual categories that don't fit (oversized) or that this adapter
/// never supports for JPEG (none currently -- every `Payload` variant maps
/// to a real JPEG marker) are reported and simply omitted; only a
/// structural failure (unparseable destination, or the re-scan/validation
/// pass below disagreeing with what was just written) rolls back the whole
/// merge to the original bytes.
pub(crate) fn embed(
    encoded: Vec<u8>,
    prepared: &MetadataBundle,
    facts: &OutputFacts,
    limits: &MetadataLimits,
) -> MetadataExport {
    let mut jpeg = match Jpeg::from_bytes(Bytes::from(encoded.clone())) {
        Ok(j) => j,
        Err(_) => return super::rollback(encoded, prepared),
    };

    let mut attempt_issues = Vec::new();
    let mut attempted: Vec<&Payload> = Vec::new();

    for payload in &prepared.payloads {
        match payload {
            Payload::Exif(bytes) => {
                if EXIF_PREFIX.len() + bytes.len() > MAX_APP_PAYLOAD {
                    attempt_issues.push(super::too_large(payload));
                } else {
                    jpeg.set_exif(Some(Bytes::from(bytes.clone())));
                    attempted.push(payload);
                }
            }
            Payload::Xmp(bytes) => {
                if XMP_STANDARD_PREFIX.len() + bytes.len() > MAX_APP_PAYLOAD {
                    attempt_issues.push(super::too_large(payload));
                } else {
                    set_xmp(&mut jpeg, bytes);
                    attempted.push(payload);
                }
            }
            Payload::Icc(bytes) => {
                jpeg.set_icc_profile(Some(Bytes::from(bytes.clone())));
                attempted.push(payload);
            }
            Payload::Iptc(bytes) => {
                let contents = build_app13(bytes);
                if contents.len() > MAX_APP_PAYLOAD {
                    attempt_issues.push(super::too_large(payload));
                } else {
                    set_app13(&mut jpeg, contents);
                    attempted.push(payload);
                }
            }
            Payload::JpegComment(bytes) => {
                if bytes.len() > MAX_APP_PAYLOAD {
                    attempt_issues.push(super::too_large(payload));
                } else {
                    set_comment(&mut jpeg, bytes);
                    attempted.push(payload);
                }
            }
            Payload::JfifDensity { units, x, y } => {
                set_jfif_density(&mut jpeg, *units, *x, *y);
                attempted.push(payload);
            }
            // No JPEG container concept for PNG-only text/density.
            Payload::PngText { .. } | Payload::PngDensity { .. } => {
                attempt_issues.push(super::unsupported(payload));
            }
        }
    }

    let mut merged = Vec::with_capacity(jpeg.len());
    if jpeg.encoder().write_to(&mut merged).is_err() {
        return super::rollback(encoded, prepared);
    }

    if !validate(&merged, &attempted, facts, limits) {
        return super::rollback(encoded, prepared);
    }

    let mut issues = prepared.report().issues.clone();
    issues.extend(attempt_issues);
    MetadataExport {
        bytes: merged,
        report: MetadataReport { issues },
    }
}

/// No `ImageEXIF`-style trait exists for XMP in `img-parts`, so this is
/// hand-rolled: remove any existing destination standard-XMP APP1 segment,
/// then insert a fresh one. `.min(len)` keeps the insertion position
/// in-bounds regardless of how few segments the destination has.
fn set_xmp(jpeg: &mut Jpeg, xmp: &[u8]) {
    jpeg.segments_mut()
        .retain(|s| !(s.marker() == markers::APP1 && s.contents().starts_with(XMP_STANDARD_PREFIX)));
    let mut contents = XMP_STANDARD_PREFIX.to_vec();
    contents.extend_from_slice(xmp);
    let pos = jpeg.segments().len().min(3);
    jpeg.segments_mut()
        .insert(pos, JpegSegment::new_with_contents(markers::APP1, Bytes::from(contents)));
}

/// Build an APP13 "Photoshop 3.0" payload wrapping a single `8BIM` IRB for
/// the IPTC-NAA record (resource `0x0404`), matching the shape
/// `crate::iptc::parse_irbs` reads back.
fn build_app13(iptc: &[u8]) -> Vec<u8> {
    let mut out = PHOTOSHOP_PREFIX.to_vec();
    out.extend_from_slice(b"8BIM");
    out.extend_from_slice(&0x0404u16.to_be_bytes());
    out.push(0); // zero-length Pascal name
    out.push(0); // pad: (1 name-length byte + 0 name bytes) is odd
    out.extend_from_slice(&(iptc.len() as u32).to_be_bytes());
    out.extend_from_slice(iptc);
    if !iptc.len().is_multiple_of(2) {
        out.push(0);
    }
    out
}

fn set_app13(jpeg: &mut Jpeg, contents: Vec<u8>) {
    jpeg.segments_mut().retain(|s| s.marker() != markers::APP13);
    let pos = jpeg.segments().len().min(3);
    jpeg.segments_mut()
        .insert(pos, JpegSegment::new_with_contents(markers::APP13, Bytes::from(contents)));
}

fn set_comment(jpeg: &mut Jpeg, data: &[u8]) {
    jpeg.segments_mut().retain(|s| s.marker() != markers::COM);
    let pos = jpeg.segments().len().min(3);
    jpeg.segments_mut()
        .insert(pos, JpegSegment::new_with_contents(markers::COM, Bytes::from(data.to_vec())));
}

/// Set the density fields of the destination's JFIF/APP0 segment: patched
/// in place if one already exists (never touching its version or
/// thumbnail-size fields, and never copying a source thumbnail or an Adobe
/// transform marker -- only the three density fields themselves move), or
/// inserted as a fresh, thumbnail-free minimal JFIF segment (version 1.1,
/// zero-size thumbnail) at the very front if the destination has none.
fn set_jfif_density(jpeg: &mut Jpeg, units: u8, x: u16, y: u16) {
    for segment in jpeg.segments_mut() {
        if segment.marker() == markers::APP0 && segment.contents().starts_with(b"JFIF\0") {
            let mut contents = segment.contents().to_vec();
            if contents.len() < 12 {
                continue;
            }
            contents[7] = units;
            contents[8..10].copy_from_slice(&x.to_be_bytes());
            contents[10..12].copy_from_slice(&y.to_be_bytes());
            *segment = JpegSegment::new_with_contents(markers::APP0, Bytes::from(contents));
            return;
        }
    }
    let mut contents = b"JFIF\0".to_vec();
    contents.extend_from_slice(&[1, 1]); // version 1.1
    contents.push(units);
    contents.extend_from_slice(&x.to_be_bytes());
    contents.extend_from_slice(&y.to_be_bytes());
    contents.extend_from_slice(&[0, 0]); // no thumbnail
    jpeg.segments_mut()
        .insert(0, JpegSegment::new_with_contents(markers::APP0, Bytes::from(contents)));
}

/// Re-scan the just-written bytes with this module's own bounded extractor
/// and confirm every payload we attempted round-trips byte-identically,
/// with no new structural damage and dimensions matching `facts`. Never
/// decodes entropy-coded pixel data.
fn validate(merged: &[u8], attempted: &[&Payload], facts: &OutputFacts, limits: &MetadataLimits) -> bool {
    let bundle = extract(merged, limits);
    if bundle
        .report()
        .issues
        .iter()
        .any(|i| i.reason == MetadataIssueReason::Malformed)
    {
        return false;
    }
    for payload in attempted {
        let present = match payload {
            Payload::Exif(b) => bundle.payloads.iter().any(|p| matches!(p, Payload::Exif(pb) if pb == b)),
            Payload::Xmp(b) => bundle.payloads.iter().any(|p| matches!(p, Payload::Xmp(pb) if pb == b)),
            Payload::Icc(b) => bundle.payloads.iter().any(|p| matches!(p, Payload::Icc(pb) if pb == b)),
            Payload::Iptc(b) => bundle.payloads.iter().any(|p| matches!(p, Payload::Iptc(pb) if pb == b)),
            Payload::JpegComment(b) => bundle
                .payloads
                .iter()
                .any(|p| matches!(p, Payload::JpegComment(pb) if pb == b)),
            Payload::JfifDensity { units, x, y } => bundle.payloads.iter().any(|p| {
                matches!(p, Payload::JfifDensity { units: u, x: px, y: py }
                    if u == units && px == x && py == y)
            }),
            Payload::PngText { .. } | Payload::PngDensity { .. } => true,
        };
        if !present {
            return false;
        }
    }
    matches!(frame_dimensions(merged), Some((w, h)) if w == facts.width && h == facts.height)
}

/// Bounded scan for the first SOF0-15 (frame header) marker's declared
/// width/height, without decoding any entropy-coded scan data. Distinct
/// from the general segment walk above because that walk only needs to
/// *find* metadata, never the frame dimensions.
fn frame_dimensions(source: &[u8]) -> Option<(u32, u32)> {
    if source.len() < 2 || source[0] != 0xFF || source[1] != 0xD8 {
        return None;
    }
    let mut pos = 2usize;
    loop {
        if pos >= source.len() || source[pos] != 0xFF {
            return None;
        }
        while pos < source.len() && source[pos] == 0xFF {
            pos += 1;
        }
        if pos >= source.len() {
            return None;
        }
        let marker = source[pos];
        pos += 1;
        if marker == MARKER_EOI || marker == MARKER_SOS {
            return None;
        }
        if marker == MARKER_TEM || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        let len = read_u16(source, pos)?;
        let seg_range = valid_segment_range(pos, len as usize, source.len())?;
        let is_sof = matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF);
        if is_sof {
            let content = &source[pos + 2..seg_range.end];
            if content.len() < 5 {
                return None;
            }
            let height = u16::from_be_bytes([content[1], content[2]]) as u32;
            let width = u16::from_be_bytes([content[3], content[4]]) as u32;
            return Some((width, height));
        }
        pos = seg_range.end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0xFF, marker];
        let len = (payload.len() + 2) as u16;
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    fn jpeg(segments: &[Vec<u8>]) -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8];
        for s in segments {
            out.extend_from_slice(s);
        }
        out.extend_from_slice(&[0xFF, MARKER_EOI]);
        out
    }

    #[test]
    fn exif_payload_starts_at_tiff_marker() {
        let mut app1 = EXIF_PREFIX.to_vec();
        app1.extend_from_slice(b"MM\0*fake tiff body");
        let data = jpeg(&[segment(0xE1, &app1)]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle.report().issues.is_empty());
        assert_eq!(bundle.format, SourceFormat::Jpeg);
        assert!(bundle.payloads.iter().any(
            |p| matches!(p, Payload::Exif(b) if b.starts_with(b"MM\0*"))
        ));
    }

    #[test]
    fn standard_xmp_is_detected() {
        let mut app1 = XMP_STANDARD_PREFIX.to_vec();
        app1.extend_from_slice(b"<x:xmpmeta>hi</x:xmpmeta>");
        let data = jpeg(&[segment(0xE1, &app1)]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle
            .payloads
            .iter()
            .any(|p| matches!(p, Payload::Xmp(b) if b.starts_with(b"<x:xmpmeta>"))));
    }

    #[test]
    fn extended_xmp_is_recognized_but_not_merged() {
        let mut app1 = XMP_EXTENDED_PREFIX.to_vec();
        app1.extend_from_slice(&[b'a'; 32]); // fake GUID
        app1.extend_from_slice(&100u32.to_be_bytes());
        app1.extend_from_slice(&0u32.to_be_bytes());
        app1.extend_from_slice(b"chunk");
        let data = jpeg(&[segment(0xE1, &app1)]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle.payloads.iter().all(|p| !matches!(p, Payload::Xmp(_))));
        assert!(bundle.report().issues.iter().any(|i| {
            i.category == MetadataCategory::Xmp
                && i.reason == MetadataIssueReason::Unsupported
                && i.field.as_deref() == Some("extended")
        }));
    }

    #[test]
    fn icc_segments_assembled_out_of_order() {
        const ICC: &[u8] = b"ICC_PROFILE\0";
        let mut seg2 = ICC.to_vec();
        seg2.extend_from_slice(&[2, 2]);
        seg2.extend_from_slice(b"WORLD");
        let mut seg1 = ICC.to_vec();
        seg1.extend_from_slice(&[1, 2]);
        seg1.extend_from_slice(b"HELLO");
        // seg2 appears before seg1 in the file.
        let data = jpeg(&[segment(0xE2, &seg2), segment(0xE2, &seg1)]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle.report().issues.is_empty());
        assert!(bundle
            .payloads
            .iter()
            .any(|p| matches!(p, Payload::Icc(b) if b == b"HELLOWORLD")));
        assert_eq!(bundle.source_color(), SourceColor::Other);
    }

    #[test]
    fn icc_missing_segment_is_malformed() {
        const ICC: &[u8] = b"ICC_PROFILE\0";
        let mut seg1 = ICC.to_vec();
        seg1.extend_from_slice(&[1, 3]);
        seg1.extend_from_slice(b"HELLO");
        // seq_no 2 of 3 is missing.
        let mut seg3 = ICC.to_vec();
        seg3.extend_from_slice(&[3, 3]);
        seg3.extend_from_slice(b"THIRD");
        let data = jpeg(&[segment(0xE2, &seg1), segment(0xE2, &seg3)]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle.payloads.iter().all(|p| !matches!(p, Payload::Icc(_))));
        assert!(bundle.report().issues.iter().any(|i| {
            i.category == MetadataCategory::Icc && i.reason == MetadataIssueReason::Malformed
        }));
    }

    #[test]
    fn icc_duplicate_sequence_number_is_malformed() {
        const ICC: &[u8] = b"ICC_PROFILE\0";
        let mut seg1a = ICC.to_vec();
        seg1a.extend_from_slice(&[1, 2]);
        seg1a.extend_from_slice(b"AAAAA");
        let mut seg1b = ICC.to_vec();
        seg1b.extend_from_slice(&[1, 2]);
        seg1b.extend_from_slice(b"BBBBB");
        let data = jpeg(&[segment(0xE2, &seg1a), segment(0xE2, &seg1b)]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle.payloads.iter().all(|p| !matches!(p, Payload::Icc(_))));
        assert!(bundle.report().issues.iter().any(|i| {
            i.category == MetadataCategory::Icc && i.reason == MetadataIssueReason::Malformed
        }));
    }

    #[test]
    fn duplicate_exif_is_dropped_as_conflicting() {
        let mut app1 = EXIF_PREFIX.to_vec();
        app1.extend_from_slice(b"MM\0*one");
        let mut app1b = EXIF_PREFIX.to_vec();
        app1b.extend_from_slice(b"MM\0*two");
        let data = jpeg(&[segment(0xE1, &app1), segment(0xE1, &app1b)]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle.payloads.iter().all(|p| !matches!(p, Payload::Exif(_))));
        assert!(bundle.report().issues.iter().any(|i| {
            i.category == MetadataCategory::Exif && i.reason == MetadataIssueReason::Malformed
        }));
    }

    #[test]
    fn com_comment_is_detected() {
        let data = jpeg(&[segment(MARKER_COM, b"a comment")]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle
            .payloads
            .iter()
            .any(|p| matches!(p, Payload::JpegComment(b) if b == b"a comment")));
    }

    #[test]
    fn jfif_density_is_detected() {
        let mut app0 = b"JFIF\0".to_vec();
        app0.extend_from_slice(&[1, 2]); // version
        app0.push(1); // units = dpi
        app0.extend_from_slice(&72u16.to_be_bytes());
        app0.extend_from_slice(&96u16.to_be_bytes());
        app0.extend_from_slice(&[0, 0]); // no thumbnail
        let data = jpeg(&[segment(0xE0, &app0)]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle.payloads.iter().any(|p| matches!(
            p,
            Payload::JfifDensity { units: 1, x: 72, y: 96 }
        )));
    }

    #[test]
    fn unknown_app_segment_is_flagged_not_copied() {
        let data = jpeg(&[segment(0xE5, b"whatever")]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle.payloads.is_empty());
        assert!(bundle.report().issues.iter().any(|i| {
            i.category == MetadataCategory::Unknown
                && i.reason == MetadataIssueReason::Unsupported
                && i.field.as_deref() == Some("APP5")
        }));
    }

    #[test]
    fn metadata_after_progressive_scan_is_found() {
        // A minimal fake progressive-scan shape: two SOS scans (with
        // stuffed-byte and restart-marker entropy data) followed by a
        // metadata segment that must still be discovered.
        let mut data = vec![0xFF, 0xD8];
        // First scan.
        data.extend(segment(MARKER_SOS, &[1, 0, 0, 0, 0, 0]));
        data.extend_from_slice(&[0x12, 0x34, 0xFF, 0x00, 0x56, 0xFF, 0xD0, 0x78]);
        // Second scan (progressive JPEGs issue multiple SOS segments).
        data.extend(segment(MARKER_SOS, &[1, 0, 0, 0, 0, 0]));
        data.extend_from_slice(&[0x9A, 0xFF, 0x00, 0xBC]);
        // Metadata placed after both scans.
        let mut app1 = EXIF_PREFIX.to_vec();
        app1.extend_from_slice(b"MM\0*late arrival");
        data.extend(segment(0xE1, &app1));
        data.extend_from_slice(&[0xFF, MARKER_EOI]);

        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle.report().issues.is_empty(), "{:?}", bundle.report());
        assert!(bundle.payloads.iter().any(
            |p| matches!(p, Payload::Exif(b) if b.starts_with(b"MM\0*late"))
        ));
    }

    #[test]
    fn truncated_segment_length_is_malformed() {
        let mut data = vec![0xFF, 0xD8, 0xFF, 0xE1, 0xFF, 0xFF]; // length claims 65535 bytes, buffer is empty
        data.truncate(6);
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle.report().issues.iter().any(|i| {
            i.reason == MetadataIssueReason::Malformed
        }));
        assert_eq!(bundle.source_color(), SourceColor::Unknown);
    }

    #[test]
    fn trailing_bytes_after_eoi_are_unverified() {
        let mut data = jpeg(&[]);
        data.extend_from_slice(b"garbage after eoi");
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle.report().issues.iter().any(|i| {
            i.reason == MetadataIssueReason::Unverified && i.field.as_deref() == Some("trailing")
        }));
    }
}
