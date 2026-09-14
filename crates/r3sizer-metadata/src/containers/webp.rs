//! WebP (RIFF) container scanning.
//!
//! Walks top-level RIFF chunks after the `RIFF....WEBP` header, validating
//! chunk lengths and the odd-size padding byte. Extracts EXIF, XMP, and
//! ICCP; recognizes VP8/VP8L/VP8X/ALPH/ANIM/ANMF as image structure and
//! skips them without recursing into their payload (in particular, no
//! recursion into generic RIFF `LIST` records).

use img_parts::riff::{RiffChunk, RiffContent};
use img_parts::webp::{
    CHUNK_EXIF as WEBP_EXIF, CHUNK_ICCP as WEBP_ICCP, CHUNK_VP8X as WEBP_VP8X,
    CHUNK_XMP as WEBP_XMP, WebP as ImgPartsWebP,
};
use img_parts::Bytes;

use crate::bundle::{MetadataBundle, Payload, SourceColor, SourceFormat};
use crate::containers::Collector;
use crate::limits::{checked_range, MetadataLimits};
use crate::types::{MetadataCategory, MetadataExport, MetadataIssueReason, MetadataReport, OutputFacts};

const EXIF_PREFIX: &[u8] = b"Exif\0\0";

/// Chunk FourCCs that are recognized WebP image structure, not metadata.
const IMAGE_STRUCTURE: &[&[u8; 4]] = &[b"VP8 ", b"VP8L", b"VP8X", b"ALPH", b"ANIM", b"ANMF"];

pub(crate) fn extract(source: &[u8], limits: &MetadataLimits) -> MetadataBundle {
    let mut collector = Collector::new(limits);
    let mut pos = 12usize; // past "RIFF" + size(4) + "WEBP"
    let mut fatal = false;

    // The RIFF size field covers everything after itself (i.e. "WEBP" +
    // chunks). Bound chunk parsing to it so bytes beyond what the header
    // declares are treated as trailing, not as more chunks.
    let declared_size =
        u32::from_le_bytes([source[4], source[5], source[6], source[7]]) as usize;
    let end_bound = match 8usize.checked_add(declared_size) {
        Some(end) if end <= source.len() => end,
        _ => {
            collector.push_issue(
                MetadataCategory::Unknown,
                MetadataIssueReason::Malformed,
                Some("riff_size".to_string()),
            );
            source.len()
        }
    };

    loop {
        if pos >= end_bound {
            break;
        }
        if pos + 8 > end_bound {
            collector.push_issue(
                MetadataCategory::Unknown,
                MetadataIssueReason::Malformed,
                Some("chunk_header_truncated".to_string()),
            );
            fatal = true;
            break;
        }
        let fourcc: [u8; 4] = [source[pos], source[pos + 1], source[pos + 2], source[pos + 3]];
        let size = u32::from_le_bytes([
            source[pos + 4],
            source[pos + 5],
            source[pos + 6],
            source[pos + 7],
        ]) as usize;
        let Some(data_range) = checked_range(pos + 8, size, 1, end_bound) else {
            collector.push_issue(
                MetadataCategory::Unknown,
                MetadataIssueReason::Malformed,
                Some("chunk_length".to_string()),
            );
            fatal = true;
            break;
        };
        let data = &source[data_range.clone()];
        let pad = size % 2;

        match &fourcc {
            b"EXIF" => collector.offer_exif(normalize_exif_prefix(data)),
            b"XMP " => collector.offer_xmp(data.to_vec()),
            b"ICCP" => collector.offer_icc(data.to_vec()),
            b"LIST" => collector.push_issue(
                MetadataCategory::Unknown,
                MetadataIssueReason::Unsupported,
                Some("LIST".to_string()),
            ),
            _ if IMAGE_STRUCTURE.contains(&&fourcc) => {}
            _ => collector.push_issue(
                MetadataCategory::Unknown,
                MetadataIssueReason::Unsupported,
                Some(String::from_utf8_lossy(&fourcc).into_owned()),
            ),
        }

        let Some(pad_range) = checked_range(data_range.end, pad, 1, end_bound) else {
            collector.push_issue(
                MetadataCategory::Unknown,
                MetadataIssueReason::Malformed,
                Some("chunk_padding".to_string()),
            );
            fatal = true;
            break;
        };
        pos = pad_range.end;
    }

    if pos < source.len() {
        collector.push_issue(
            MetadataCategory::Unknown,
            MetadataIssueReason::Unverified,
            Some("trailing".to_string()),
        );
    }

    let mut bundle = collector.finish(SourceFormat::WebP);
    let icc_present = bundle.payloads.iter().any(|p| matches!(p, Payload::Icc(_)));
    bundle.source_color = if icc_present {
        SourceColor::Other
    } else if fatal {
        SourceColor::Unknown
    } else {
        SourceColor::Unspecified
    };
    bundle
}

/// Some encoders wrap the EXIF chunk payload with the JPEG-style
/// `Exif\0\0` identifier even though WebP's EXIF chunk is specified to
/// start directly at the TIFF header; strip it if present.
fn normalize_exif_prefix(data: &[u8]) -> Vec<u8> {
    match data.strip_prefix(EXIF_PREFIX) {
        Some(rest) => rest.to_vec(),
        None => data.to_vec(),
    }
}

// --- Merge / embed ---------------------------------------------------------
//
// `img-parts` 0.4.0's own `WebP::set_exif`/`set_icc_profile` are not used
// here: their `convert_into_infered_kind` only ever infers VP8 <-> VP8X
// (there's a literal `// TODO: VP8L` where alpha/animation handling would
// go), `WebPFlags::from_webp` never sets the alpha or animation bits at
// all, there's no XMP support whatsoever, and it calls `self.dimensions()`
// (which can panic on a malformed VP8 keyframe -- see `vp8::size_from_vp8_header`)
// on every conversion. Chunks are built and reordered by hand instead,
// using `chunks_mut()`/`RiffChunk::new` directly, and dimensions/flags come
// from this module's own bounded, non-panicking `scan()`.

const FLAG_ICC: u8 = 0b0010_0000;
const FLAG_ALPHA: u8 = 0b0001_0000;
const FLAG_EXIF: u8 = 0b0000_1000;
const FLAG_XMP: u8 = 0b0000_0100;
const FLAG_ANIM: u8 = 0b0000_0010;

/// Checked VP8X payload construction (brief-specified): `None` if the
/// canvas dimensions can't be represented in VP8X's 24-bit fields.
fn vp8x_header(width: u32, height: u32, flags: u8) -> Option<[u8; 10]> {
    if width == 0 || height == 0 || width > 0x0100_0000 || height > 0x0100_0000 {
        return None;
    }
    let mut data = [0u8; 10];
    data[0] = flags;
    data[4..7].copy_from_slice(&(width - 1).to_le_bytes()[..3]);
    data[7..10].copy_from_slice(&(height - 1).to_le_bytes()[..3]);
    Some(data)
}

fn u24_le(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], 0])
}

/// Bounded, non-panicking read of a VP8 (lossy) keyframe's declared
/// dimensions from its first 10 bytes. Returns `None` on anything that
/// doesn't look like a valid keyframe rather than panicking, unlike
/// `img_parts::vp8::size_from_vp8_header`.
fn vp8_keyframe_dims(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 10 {
        return None;
    }
    let tag = u24_le(&data[0..3]);
    if tag & 1 != 0 {
        return None; // not a keyframe
    }
    if data[3..6] != [0x9d, 0x01, 0x2a] {
        return None;
    }
    let width = (u16::from_le_bytes([data[6], data[7]]) & 0x3FFF) as u32;
    let height = (u16::from_le_bytes([data[8], data[9]]) & 0x3FFF) as u32;
    Some((width, height))
}

/// A VP8L (lossless) bitstream's dimensions and alpha-used bit, read
/// directly from its 5-byte header per the WebP Lossless Format spec.
fn vp8l_header(data: &[u8]) -> Option<((u32, u32), bool)> {
    if data.len() < 5 || data[0] != 0x2f {
        return None;
    }
    let bits = u32::from_le_bytes(data[1..5].try_into().unwrap());
    let width = (bits & 0x3FFF) + 1;
    let height = ((bits >> 14) & 0x3FFF) + 1;
    let alpha = (bits >> 28) & 1 == 1;
    Some(((width, height), alpha))
}

/// Result of a bounded top-level scan of a WebP container's own bytes:
/// used both to read the *destination*'s current shape before mutating it,
/// and to re-verify the merged bytes afterward.
struct Scan {
    order: Vec<[u8; 4]>,
    dims: Option<(u32, u32)>,
    has_alpha: bool,
    has_anim: bool,
}

/// Bounded top-level RIFF chunk walk (mirrors `extract`'s loop, but this
/// one also collects ordering/dimensions/flags rather than payloads, and is
/// used on both source and destination bytes). Returns `None` on any
/// malformed framing or a second `VP8X` chunk.
fn scan(source: &[u8]) -> Option<Scan> {
    if source.len() < 12 || &source[0..4] != b"RIFF" || &source[8..12] != b"WEBP" {
        return None;
    }
    let declared_size = u32::from_le_bytes([source[4], source[5], source[6], source[7]]) as usize;
    let end = 8usize.checked_add(declared_size)?;
    if end > source.len() {
        return None;
    }

    let mut pos = 12usize;
    let mut order = Vec::new();
    let mut vp8x_dims = None;
    let mut vp8_dims = None;
    let mut vp8l_dims = None;
    let mut vp8l_alpha = false;
    let mut has_alph_chunk = false;
    let mut has_anim = false;
    let mut vp8x_count = 0u32;

    while pos < end {
        if pos + 8 > end {
            return None;
        }
        let fourcc: [u8; 4] = [source[pos], source[pos + 1], source[pos + 2], source[pos + 3]];
        let size = u32::from_le_bytes([source[pos + 4], source[pos + 5], source[pos + 6], source[pos + 7]])
            as usize;
        let data_range = checked_range(pos + 8, size, 1, end)?;
        let data = &source[data_range.clone()];
        order.push(fourcc);

        if fourcc == WEBP_VP8X {
            vp8x_count += 1;
            if vp8x_count > 1 || data.len() != 10 {
                return None;
            }
            vp8x_dims = Some((u24_le(&data[4..7]) + 1, u24_le(&data[7..10]) + 1));
        } else if fourcc == *b"ALPH" {
            has_alph_chunk = true;
        } else if fourcc == *b"ANIM" {
            has_anim = true;
        } else if fourcc == *b"VP8 " {
            vp8_dims = vp8_keyframe_dims(data);
        } else if fourcc == *b"VP8L" {
            if let Some((dims, alpha)) = vp8l_header(data) {
                vp8l_dims = Some(dims);
                vp8l_alpha = alpha;
            }
        }

        let pad = size % 2;
        pos = checked_range(data_range.end, pad, 1, end)?.end;
    }

    Some(Scan {
        order,
        dims: vp8x_dims.or(vp8_dims).or(vp8l_dims),
        has_alpha: has_alph_chunk || vp8l_alpha,
        has_anim,
    })
}

/// Insert `prepared`'s payloads into an already-encoded destination WebP.
///
/// EXIF/XMP/ICC always survive; anything this crate has no WebP container
/// concept for (IPTC, JPEG comment, JFIF/PNG density, PNG text) is
/// `Unsupported`. A `VP8X` chunk is added only when EXIF/ICC/XMP actually
/// requires one; existing alpha/animation flags are always carried over
/// unchanged (this adapter never touches image data).
pub(crate) fn embed(
    encoded: Vec<u8>,
    prepared: &MetadataBundle,
    facts: &OutputFacts,
    limits: &MetadataLimits,
) -> MetadataExport {
    let Some(original) = scan(&encoded) else {
        return super::rollback(encoded, prepared);
    };
    let mut webp = match ImgPartsWebP::from_bytes(Bytes::from(encoded.clone())) {
        Ok(w) => w,
        Err(_) => return super::rollback(encoded, prepared),
    };

    let existing = std::mem::take(webp.chunks_mut());
    let mut body: Vec<RiffChunk> = Vec::new();
    let mut final_icc: Option<Vec<u8>> = None;
    for c in existing {
        let id = c.id();
        if id == WEBP_ICCP {
            final_icc = c.content().data().map(|d| d.to_vec());
        } else if id != WEBP_VP8X && id != WEBP_EXIF && id != WEBP_XMP {
            body.push(c);
        }
        // WEBP_VP8X/WEBP_EXIF/WEBP_XMP chunks are dropped: regenerated below.
    }

    let mut attempt_issues = Vec::new();
    let mut attempted: Vec<&Payload> = Vec::new();
    let mut final_exif: Option<Vec<u8>> = None;
    let mut final_xmp: Option<Vec<u8>> = None;

    for payload in &prepared.payloads {
        match payload {
            Payload::Exif(bytes) => {
                if bytes.len() > limits.max_payload_bytes {
                    attempt_issues.push(super::too_large(payload));
                } else {
                    final_exif = Some(bytes.clone());
                    attempted.push(payload);
                }
            }
            Payload::Xmp(bytes) => {
                if bytes.len() > limits.max_payload_bytes {
                    attempt_issues.push(super::too_large(payload));
                } else {
                    final_xmp = Some(bytes.clone());
                    attempted.push(payload);
                }
            }
            Payload::Icc(bytes) => {
                final_icc = Some(bytes.clone());
                attempted.push(payload);
            }
            Payload::Iptc(_)
            | Payload::JpegComment(_)
            | Payload::JfifDensity { .. }
            | Payload::PngText { .. }
            | Payload::PngDensity { .. } => {
                attempt_issues.push(super::unsupported(payload));
            }
        }
    }

    let flags = (if final_icc.is_some() { FLAG_ICC } else { 0 })
        | (if final_exif.is_some() { FLAG_EXIF } else { 0 })
        | (if final_xmp.is_some() { FLAG_XMP } else { 0 })
        | (if original.has_alpha { FLAG_ALPHA } else { 0 })
        | (if original.has_anim { FLAG_ANIM } else { 0 });

    let mut new_chunks: Vec<RiffChunk> = Vec::new();
    if flags != 0 {
        // EXIF/ICC/XMP (or pre-existing alpha/animation) requires a VP8X
        // header, which needs the real canvas dimensions.
        let Some((width, height)) = original.dims else {
            return super::rollback(encoded, prepared);
        };
        let Some(header) = vp8x_header(width, height, flags) else {
            return super::rollback(encoded, prepared);
        };
        new_chunks.push(RiffChunk::new(WEBP_VP8X, RiffContent::Data(Bytes::copy_from_slice(&header))));
    }
    if let Some(icc) = &final_icc {
        new_chunks.push(RiffChunk::new(WEBP_ICCP, RiffContent::Data(Bytes::from(icc.clone()))));
    }
    new_chunks.extend(body);
    if let Some(exif) = &final_exif {
        new_chunks.push(RiffChunk::new(WEBP_EXIF, RiffContent::Data(Bytes::from(exif.clone()))));
    }
    if let Some(xmp) = &final_xmp {
        new_chunks.push(RiffChunk::new(WEBP_XMP, RiffContent::Data(Bytes::from(xmp.clone()))));
    }
    *webp.chunks_mut() = new_chunks;

    let mut merged = Vec::with_capacity(webp.len() as usize);
    if webp.encoder().write_to(&mut merged).is_err() {
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

/// `VP8X` (if present) must be the first chunk, `ICCP` (if present) must
/// immediately follow it, and `EXIF`/`XMP` (in that order, if present) must
/// come after every other chunk -- the ordering this adapter always
/// produces. `scan()` already rejects a second `VP8X`.
fn valid_order(order: &[[u8; 4]]) -> bool {
    let mut idx = 0usize;
    if order.first() == Some(&WEBP_VP8X) {
        idx = 1;
        if order.get(1) == Some(&WEBP_ICCP) {
            idx = 2;
        }
    } else if order.contains(&WEBP_ICCP) {
        return false; // ICCP without a leading VP8X is never what this adapter writes
    }

    let mut seen_exif = false;
    let mut seen_xmp = false;
    for &id in &order[idx..] {
        if id == WEBP_VP8X || id == WEBP_ICCP {
            return false;
        } else if id == WEBP_EXIF {
            if seen_xmp {
                return false;
            }
            seen_exif = true;
        } else if id == WEBP_XMP {
            seen_xmp = true;
        } else if seen_exif || seen_xmp {
            return false;
        }
    }
    true
}

/// Re-scan the just-written bytes with this module's own bounded extractor
/// and confirm every payload we attempted round-trips byte-identically,
/// chunk ordering/flags are legal, and dimensions match `facts`.
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
            _ => true,
        };
        if !present {
            return false;
        }
    }
    let Some(rescan) = scan(merged) else {
        return false;
    };
    if !valid_order(&rescan.order) {
        return false;
    }
    matches!(rescan.dims, Some((w, h)) if w == facts.width && h == facts.height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MetadataIssueReason;

    fn chunk(fourcc: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = fourcc.to_vec();
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
        if !data.len().is_multiple_of(2) {
            out.push(0);
        }
        out
    }

    fn webp(chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut body = b"WEBP".to_vec();
        for c in chunks {
            body.extend_from_slice(c);
        }
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn exif_and_xmp_are_detected() {
        let data = webp(&[
            chunk(b"EXIF", b"MM\0*tiff body"),
            chunk(b"XMP ", b"<x:xmpmeta/>"),
        ]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle.report().issues.is_empty(), "{:?}", bundle.report());
        assert!(bundle
            .payloads
            .iter()
            .any(|p| matches!(p, Payload::Exif(b) if b == b"MM\0*tiff body")));
        assert!(bundle
            .payloads
            .iter()
            .any(|p| matches!(p, Payload::Xmp(b) if b == b"<x:xmpmeta/>")));
    }

    #[test]
    fn exif_identifier_prefix_is_normalized() {
        let mut prefixed = EXIF_PREFIX.to_vec();
        prefixed.extend_from_slice(b"MM\0*tiff body");
        let data = webp(&[chunk(b"EXIF", &prefixed)]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle
            .payloads
            .iter()
            .any(|p| matches!(p, Payload::Exif(b) if b == b"MM\0*tiff body")));
    }

    #[test]
    fn odd_size_xmp_chunk_padding_is_handled() {
        // Odd-length XMP payload forces a pad byte; a later chunk must
        // still parse correctly, proving the pad byte was skipped.
        let data = webp(&[
            chunk(b"XMP ", b"<a/>"), // 4 bytes, even, no padding needed on its own
            chunk(b"EXIF", b"MM\0*odd"), // 7 bytes: odd, needs padding
            chunk(b"ICCP", b"profile-bytes"),
        ]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle.report().issues.is_empty(), "{:?}", bundle.report());
        assert!(bundle
            .payloads
            .iter()
            .any(|p| matches!(p, Payload::Icc(b) if b == b"profile-bytes")));
    }

    #[test]
    fn iccp_is_detected_and_sets_source_color() {
        let data = webp(&[chunk(b"ICCP", b"a profile")]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert_eq!(bundle.source_color(), SourceColor::Other);
    }

    #[test]
    fn unknown_chunk_is_flagged_not_copied() {
        let data = webp(&[chunk(b"FOOO", b"mystery")]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle.payloads.is_empty());
        assert!(bundle.report().issues.iter().any(|i| {
            i.category == MetadataCategory::Unknown
                && i.reason == MetadataIssueReason::Unsupported
                && i.field.as_deref() == Some("FOOO")
        }));
    }

    #[test]
    fn list_chunk_is_not_recursed_into() {
        // A LIST chunk that itself contains what looks like an EXIF
        // sub-chunk; it must not be extracted since we don't recurse.
        let mut inner = b"WEBPEXIF".to_vec();
        inner.extend_from_slice(&4u32.to_le_bytes());
        inner.extend_from_slice(b"body");
        let data = webp(&[chunk(b"LIST", &inner)]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle.payloads.iter().all(|p| !matches!(p, Payload::Exif(_))));
        assert!(bundle.report().issues.iter().any(|i| {
            i.category == MetadataCategory::Unknown
                && i.reason == MetadataIssueReason::Unsupported
                && i.field.as_deref() == Some("LIST")
        }));
    }

    #[test]
    fn truncated_chunk_is_malformed() {
        let mut data = b"RIFF".to_vec();
        data.extend_from_slice(&20u32.to_le_bytes());
        data.extend_from_slice(b"WEBP");
        data.extend_from_slice(b"EXIF");
        data.extend_from_slice(&1000u32.to_le_bytes()); // claims far more than remains
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle
            .report()
            .issues
            .iter()
            .any(|i| i.reason == MetadataIssueReason::Malformed));
        assert_eq!(bundle.source_color(), SourceColor::Unknown);
    }

    #[test]
    fn duplicate_xmp_is_dropped_as_conflicting() {
        let data = webp(&[chunk(b"XMP ", b"<one/>"), chunk(b"XMP ", b"<two/>")]);
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle.payloads.iter().all(|p| !matches!(p, Payload::Xmp(_))));
        assert!(bundle.report().issues.iter().any(|i| {
            i.category == MetadataCategory::Xmp && i.reason == MetadataIssueReason::Malformed
        }));
    }

    #[test]
    fn trailing_bytes_are_unverified() {
        let mut data = webp(&[]);
        data.extend_from_slice(b"trailing junk");
        let bundle = extract(&data, &MetadataLimits::default());
        assert!(bundle.report().issues.iter().any(|i| {
            i.reason == MetadataIssueReason::Unverified && i.field.as_deref() == Some("trailing")
        }));
    }
}
