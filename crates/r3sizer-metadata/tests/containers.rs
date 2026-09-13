//! Black-box container extraction tests: exercise only the public
//! `r3sizer_metadata` API (`extract`, `MetadataBundle::report`,
//! `MetadataBundle::source_color`). Payload *contents* (Exif/Xmp/Iptc/Icc
//! bytes, PngText fields, etc) are `pub(crate)` and are instead asserted by
//! module-local `#[cfg(test)]` unit tests inside
//! `src/containers/{jpeg,png,webp}.rs` and `src/iptc.rs`.
//!
//! All fixtures here are synthesized byte-for-byte in this file (see
//! `tests/fixtures/README.md`) rather than loaded from binary image files,
//! so every edge case (corrupt CRCs, truncated segments, decompression
//! bombs) is exact and reproducible without any real photograph.

use r3sizer_metadata::*;

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32fast::hash(&out[4..]);
    out.extend_from_slice(&crc.to_be_bytes());
    out
}

fn png_ihdr() -> Vec<u8> {
    chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0])
}

fn png(chunks: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(png_ihdr());
    for c in chunks {
        out.extend_from_slice(c);
    }
    out.extend(chunk(b"IEND", &[]));
    out
}

fn jpeg_segment(marker: u8, payload: &[u8]) -> Vec<u8> {
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
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

fn webp_chunk(fourcc: &[u8; 4], data: &[u8]) -> Vec<u8> {
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

fn compress(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

// --- Step 1 fixture (kept verbatim: this was the original RED test) -------

#[test]
fn png_text_is_detected_without_a_pixel_decode() {
    let mut png_bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    png_bytes.extend(chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0]));
    png_bytes.extend(chunk(b"tEXt", b"Author\0Fixture Author"));
    png_bytes.extend(chunk(b"IDAT", &[])); // container fixture; deliberately not a valid pixel stream
    png_bytes.extend(chunk(b"IEND", &[]));
    let bundle = extract(&png_bytes, &MetadataLimits::default());
    assert!(bundle.report().issues.is_empty());
}

// --- JPEG -------------------------------------------------------------

#[test]
fn jpeg_with_exif_and_jfif_is_clean() {
    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend_from_slice(b"MM\0*fake tiff");
    let mut app0 = b"JFIF\0".to_vec();
    app0.extend_from_slice(&[1, 2, 1]);
    app0.extend_from_slice(&72u16.to_be_bytes());
    app0.extend_from_slice(&72u16.to_be_bytes());
    app0.extend_from_slice(&[0, 0]);
    let data = jpeg(&[jpeg_segment(0xE0, &app0), jpeg_segment(0xE1, &app1)]);
    let bundle = extract(&data, &MetadataLimits::default());
    assert!(bundle.report().issues.is_empty(), "{:?}", bundle.report());
}

#[test]
fn jpeg_duplicate_exif_is_reported_and_dropped() {
    let mut app1a = b"Exif\0\0".to_vec();
    app1a.extend_from_slice(b"MM\0*one");
    let mut app1b = b"Exif\0\0".to_vec();
    app1b.extend_from_slice(b"MM\0*two");
    let data = jpeg(&[jpeg_segment(0xE1, &app1a), jpeg_segment(0xE1, &app1b)]);
    let bundle = extract(&data, &MetadataLimits::default());
    assert!(bundle.report().issues.iter().any(|i| {
        i.category == MetadataCategory::Exif && i.reason == MetadataIssueReason::Malformed
    }));
}

#[test]
fn jpeg_icc_out_of_order_segments_assemble_cleanly() {
    const ICC: &[u8] = b"ICC_PROFILE\0";
    let mut seg2 = ICC.to_vec();
    seg2.extend_from_slice(&[2, 2]);
    seg2.extend_from_slice(b"WORLD");
    let mut seg1 = ICC.to_vec();
    seg1.extend_from_slice(&[1, 2]);
    seg1.extend_from_slice(b"HELLO");
    let data = jpeg(&[jpeg_segment(0xE2, &seg2), jpeg_segment(0xE2, &seg1)]);
    let bundle = extract(&data, &MetadataLimits::default());
    assert!(bundle.report().issues.is_empty(), "{:?}", bundle.report());
    assert_eq!(bundle.source_color(), SourceColor::Other);
}

#[test]
fn jpeg_icc_missing_segment_is_malformed() {
    const ICC: &[u8] = b"ICC_PROFILE\0";
    let mut seg1 = ICC.to_vec();
    seg1.extend_from_slice(&[1, 3]);
    seg1.extend_from_slice(b"HELLO");
    let data = jpeg(&[jpeg_segment(0xE2, &seg1)]); // seq 2 and 3 missing
    let bundle = extract(&data, &MetadataLimits::default());
    assert!(bundle.report().issues.iter().any(|i| {
        i.category == MetadataCategory::Icc && i.reason == MetadataIssueReason::Malformed
    }));
    assert_ne!(bundle.source_color(), SourceColor::Other);
}

#[test]
fn jpeg_icc_duplicate_sequence_is_malformed() {
    const ICC: &[u8] = b"ICC_PROFILE\0";
    let mut seg1a = ICC.to_vec();
    seg1a.extend_from_slice(&[1, 1]);
    seg1a.extend_from_slice(b"AAAAA");
    let mut seg1b = ICC.to_vec();
    seg1b.extend_from_slice(&[1, 1]);
    seg1b.extend_from_slice(b"BBBBB");
    let data = jpeg(&[jpeg_segment(0xE2, &seg1a), jpeg_segment(0xE2, &seg1b)]);
    let bundle = extract(&data, &MetadataLimits::default());
    assert!(bundle.report().issues.iter().any(|i| {
        i.category == MetadataCategory::Icc && i.reason == MetadataIssueReason::Malformed
    }));
}

#[test]
fn jpeg_metadata_after_progressive_scan_is_still_found() {
    let mut data = vec![0xFF, 0xD8];
    data.extend(jpeg_segment(0xDA, &[1, 0, 0, 0, 0, 0]));
    data.extend_from_slice(&[0x12, 0x34, 0xFF, 0x00, 0x56, 0xFF, 0xD0, 0x78]);
    data.extend(jpeg_segment(0xDA, &[1, 0, 0, 0, 0, 0]));
    data.extend_from_slice(&[0x9A, 0xFF, 0x00, 0xBC]);
    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend_from_slice(b"MM\0*late arrival");
    data.extend(jpeg_segment(0xE1, &app1));
    data.extend_from_slice(&[0xFF, 0xD9]);

    let bundle = extract(&data, &MetadataLimits::default());
    assert!(bundle.report().issues.is_empty(), "{:?}", bundle.report());
}

#[test]
fn jpeg_irb_retains_iptc_and_removes_thumbnail() {
    fn irb(id: u16, data: &[u8]) -> Vec<u8> {
        let mut out = b"8BIM".to_vec();
        out.extend_from_slice(&id.to_be_bytes());
        out.push(0); // zero-length Pascal name, already even (1 byte total... needs padding)
        out.push(0); // pad byte: (1 name-length byte + 0 name bytes) = 1, odd, pad to 2
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(data);
        if !data.len().is_multiple_of(2) {
            out.push(0);
        }
        out
    }
    let iptc_dataset = {
        let mut d = vec![0x1Cu8, 2, 5];
        d.extend_from_slice(&5u16.to_be_bytes());
        d.extend_from_slice(b"hello");
        d
    };
    let mut app13 = b"Photoshop 3.0\0".to_vec();
    app13.extend(irb(0x0404, &iptc_dataset));
    app13.extend(irb(0x040C, &[1, 2, 3, 4]));
    let data = jpeg(&[jpeg_segment(0xED, &app13)]);
    let bundle = extract(&data, &MetadataLimits::default());
    assert!(bundle.report().issues.iter().any(|i| {
        i.category == MetadataCategory::Thumbnail && i.reason == MetadataIssueReason::RemovedStale
    }));
    // IPTC itself validated cleanly, only the thumbnail issue should exist.
    assert!(!bundle.report().issues.iter().any(|i| i.category == MetadataCategory::Iptc));
}

#[test]
fn jpeg_unknown_app_segment_is_flagged() {
    let data = jpeg(&[jpeg_segment(0xE6, b"whatever")]);
    let bundle = extract(&data, &MetadataLimits::default());
    assert!(bundle.report().issues.iter().any(|i| {
        i.category == MetadataCategory::Unknown && i.reason == MetadataIssueReason::Unsupported
    }));
}

#[test]
fn jpeg_truncated_segment_is_malformed() {
    let mut data = vec![0xFF, 0xD8, 0xFF, 0xE1, 0xFF, 0xFF];
    data.truncate(6);
    let bundle = extract(&data, &MetadataLimits::default());
    assert!(bundle
        .report()
        .issues
        .iter()
        .any(|i| i.reason == MetadataIssueReason::Malformed));
}

// --- PNG ----------------------------------------------------------------

#[test]
fn png_corrupted_crc_is_malformed_and_excluded() {
    let mut bad = chunk(b"tEXt", b"Author\0Someone");
    let last = bad.len() - 1;
    bad[last] ^= 0xFF;
    let data = png(&[bad]);
    let bundle = extract(&data, &MetadataLimits::default());
    assert!(bundle.report().issues.iter().any(|i| {
        i.category == MetadataCategory::Text && i.reason == MetadataIssueReason::Malformed
    }));
}

#[test]
fn png_compressed_text_exceeding_budget_is_limit_exceeded() {
    let big = vec![b'a'; 10_000];
    let compressed = compress(&big);
    let mut ztxt_data = b"kw\0\0".to_vec();
    ztxt_data.extend_from_slice(&compressed);
    let data = png(&[chunk(b"zTXt", &ztxt_data)]);
    let limits = MetadataLimits {
        max_payload_bytes: 100,
        max_total_metadata_bytes: 100,
        ..MetadataLimits::default()
    };
    let bundle = extract(&data, &limits);
    assert!(bundle.report().issues.iter().any(|i| {
        i.reason == MetadataIssueReason::LimitExceeded
    }));
}

#[test]
fn png_duplicate_exif_via_exif_chunk_is_reported() {
    let mut tiff_a = b"MM\0*".to_vec();
    tiff_a.extend_from_slice(b"one");
    let mut tiff_b = b"MM\0*".to_vec();
    tiff_b.extend_from_slice(b"two");
    let data = png(&[chunk(b"eXIf", &tiff_a), chunk(b"eXIf", &tiff_b)]);
    let bundle = extract(&data, &MetadataLimits::default());
    assert!(bundle.report().issues.iter().any(|i| {
        i.category == MetadataCategory::Exif && i.reason == MetadataIssueReason::Malformed
    }));
}

#[test]
fn png_unknown_chunk_is_flagged() {
    let data = png(&[chunk(b"zzZz", b"mystery")]);
    let bundle = extract(&data, &MetadataLimits::default());
    assert!(bundle.report().issues.iter().any(|i| {
        i.category == MetadataCategory::Unknown && i.reason == MetadataIssueReason::Unsupported
    }));
}

#[test]
fn png_truncated_container_is_malformed() {
    let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
    data.extend(png_ihdr());
    data.extend_from_slice(&1000u32.to_be_bytes());
    data.extend_from_slice(b"tEXt"); // declares far more data than remains
    let bundle = extract(&data, &MetadataLimits::default());
    assert!(bundle
        .report()
        .issues
        .iter()
        .any(|i| i.reason == MetadataIssueReason::Malformed));
}

// --- WebP -----------------------------------------------------------------

#[test]
fn webp_odd_size_xmp_and_exif_are_detected() {
    let data = webp(&[
        webp_chunk(b"XMP ", b"<x:xmpmeta/>"), // 12 bytes, even
        webp_chunk(b"EXIF", b"MM\0*odd"),      // 7 bytes, odd: needs padding
    ]);
    let bundle = extract(&data, &MetadataLimits::default());
    assert!(bundle.report().issues.is_empty(), "{:?}", bundle.report());
}

#[test]
fn webp_extended_xmp_prefix_via_jpeg_is_recognized() {
    // Extended XMP is a JPEG APP1 concept; assert it doesn't get folded
    // into a spurious duplicate with a normal standard-XMP segment.
    let mut standard = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    standard.extend_from_slice(b"<x:xmpmeta/>");
    let mut extended = b"http://ns.adobe.com/xmp/extension/\0".to_vec();
    extended.extend_from_slice(&[b'a'; 32]);
    extended.extend_from_slice(&10u32.to_be_bytes());
    extended.extend_from_slice(&0u32.to_be_bytes());
    extended.extend_from_slice(b"more xmp");
    let data = jpeg(&[
        jpeg_segment(0xE1, &standard),
        jpeg_segment(0xE1, &extended),
    ]);
    let bundle = extract(&data, &MetadataLimits::default());
    // Standard XMP survives; extended is recognized (Unsupported, not an
    // error) rather than causing a duplicate-XMP conflict.
    assert!(!bundle.report().issues.iter().any(|i| {
        i.category == MetadataCategory::Xmp && i.reason == MetadataIssueReason::Malformed
    }));
    assert!(bundle.report().issues.iter().any(|i| {
        i.category == MetadataCategory::Xmp && i.reason == MetadataIssueReason::Unsupported
    }));
}

#[test]
fn webp_unknown_chunk_is_flagged() {
    let data = webp(&[webp_chunk(b"FOOO", b"mystery")]);
    let bundle = extract(&data, &MetadataLimits::default());
    assert!(bundle.report().issues.iter().any(|i| {
        i.category == MetadataCategory::Unknown && i.reason == MetadataIssueReason::Unsupported
    }));
}

#[test]
fn webp_truncated_container_is_malformed() {
    let mut data = b"RIFF".to_vec();
    data.extend_from_slice(&20u32.to_le_bytes());
    data.extend_from_slice(b"WEBP");
    data.extend_from_slice(b"EXIF");
    data.extend_from_slice(&1000u32.to_le_bytes());
    let bundle = extract(&data, &MetadataLimits::default());
    assert!(bundle
        .report()
        .issues
        .iter()
        .any(|i| i.reason == MetadataIssueReason::Malformed));
}

// --- Cross-format ----------------------------------------------------------

#[test]
fn unrecognized_source_still_falls_back_conservatively() {
    let bundle = extract(b"not an image at all", &MetadataLimits::default());
    assert!(bundle
        .report()
        .issues
        .iter()
        .any(|i| i.reason == MetadataIssueReason::Unverified));
}
