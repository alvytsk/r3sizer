//! WebP (RIFF) container scanning.
//!
//! Walks top-level RIFF chunks after the `RIFF....WEBP` header, validating
//! chunk lengths and the odd-size padding byte. Extracts EXIF, XMP, and
//! ICCP; recognizes VP8/VP8L/VP8X/ALPH/ANIM/ANMF as image structure and
//! skips them without recursing into their payload (in particular, no
//! recursion into generic RIFF `LIST` records).

use crate::bundle::{MetadataBundle, Payload, SourceColor, SourceFormat};
use crate::containers::Collector;
use crate::limits::{checked_range, MetadataLimits};
use crate::types::{MetadataCategory, MetadataIssueReason};

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
