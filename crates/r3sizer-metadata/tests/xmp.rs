//! Black-box tests for the color/density preservation policy
//! (`r3sizer_metadata::prepare`) driven end-to-end through the public
//! `extract` -> `prepare` -> `MetadataBundle::report` surface.
//!
//! `xmp::correct`'s own namespace/XML-shape behavior (aliased prefixes,
//! arrays, thumbnails, malformed XML, depth limits, ...) is `pub(crate)`
//! and is instead covered by the module-local `#[cfg(test)]` tests inside
//! `src/xmp.rs`. This file exercises what's only reachable from outside the
//! crate: `prepare`'s ICC color-policy decisions, that density payloads are
//! left alone, and that its issue list is deduplicated once corrections
//! (which can repeat an issue extraction already recorded, or repeat one
//! across multiple occurrences of the same stale XMP property) are merged
//! in.

use r3sizer_metadata::*;

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

fn xmp_segment(xml: &str) -> Vec<u8> {
    let mut app1 = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    app1.extend_from_slice(xml.as_bytes());
    jpeg_segment(0xE1, &app1)
}

fn icc_segment(profile: &[u8]) -> Vec<u8> {
    let mut app2 = b"ICC_PROFILE\0".to_vec();
    app2.extend_from_slice(&[1, 1]); // sequence 1 of 1
    app2.extend_from_slice(profile);
    jpeg_segment(0xE2, &app2)
}

fn valid_icc(byte: u8) -> Vec<u8> {
    let mut b = vec![byte; 132];
    let size = (b.len() as u32).to_be_bytes();
    b[0..4].copy_from_slice(&size);
    b[36..40].copy_from_slice(b"acsp");
    b[128..132].copy_from_slice(&0u32.to_be_bytes());
    b
}

fn facts(width: u32, height: u32, color: ColorAction) -> OutputFacts {
    OutputFacts {
        width,
        height,
        orientation: OrientationAction::Preserve,
        color,
    }
}

const RDF_OPEN: &str = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:tiff="http://ns.adobe.com/tiff/1.0/">"#;

#[test]
fn xmp_dimensions_are_corrected_through_prepare() {
    let xml = format!(
        "{RDF_OPEN}<rdf:Description><tiff:ImageWidth>4000</tiff:ImageWidth><tiff:ImageLength>3000</tiff:ImageLength></rdf:Description></rdf:RDF>"
    );
    let data = jpeg(&[xmp_segment(&xml)]);
    let bundle = extract(&data, &MetadataLimits::default());
    let prepared = prepare(
        &bundle,
        &facts(800, 600, ColorAction::Unchanged),
        None,
        &MetadataLimits::default(),
    );
    assert!(prepared.report().issues.is_empty(), "{:?}", prepared.report());
}

#[test]
fn srgb_keeps_icc_equal_to_destination() {
    let dest = valid_icc(7);
    let data = jpeg(&[icc_segment(&dest)]);
    let bundle = extract(&data, &MetadataLimits::default());
    let prepared = prepare(
        &bundle,
        &facts(100, 50, ColorAction::Srgb),
        Some(&dest),
        &MetadataLimits::default(),
    );
    assert!(prepared.report().issues.is_empty(), "{:?}", prepared.report());
}

#[test]
fn srgb_drops_icc_that_differs_from_destination() {
    let data = jpeg(&[icc_segment(&valid_icc(1))]);
    let bundle = extract(&data, &MetadataLimits::default());
    let prepared = prepare(
        &bundle,
        &facts(100, 50, ColorAction::Srgb),
        Some(&valid_icc(2)),
        &MetadataLimits::default(),
    );
    assert!(prepared.report().issues.iter().any(|i| {
        i.category == MetadataCategory::Icc && i.reason == MetadataIssueReason::Unverified
    }));
}

#[test]
fn unverified_drops_icc_even_when_structurally_valid() {
    let icc = valid_icc(3);
    let data = jpeg(&[icc_segment(&icc)]);
    let bundle = extract(&data, &MetadataLimits::default());
    let prepared = prepare(
        &bundle,
        &facts(100, 50, ColorAction::Unverified),
        Some(&icc),
        &MetadataLimits::default(),
    );
    assert!(prepared.report().issues.iter().any(|i| {
        i.category == MetadataCategory::Icc && i.reason == MetadataIssueReason::Unverified
    }));
}

#[test]
fn malformed_icc_profile_is_reported() {
    // Too short to even hold a header: extraction still inventories the
    // bytes (it doesn't interpret them), but `prepare`'s structural
    // validation must catch it.
    let data = jpeg(&[icc_segment(&[0xAB; 4])]);
    let bundle = extract(&data, &MetadataLimits::default());
    let prepared = prepare(
        &bundle,
        &facts(100, 50, ColorAction::Unchanged),
        None,
        &MetadataLimits::default(),
    );
    assert!(prepared.report().issues.iter().any(|i| {
        i.category == MetadataCategory::Icc && i.reason == MetadataIssueReason::Malformed
    }));
}

#[test]
fn density_is_unchanged_by_a_large_downscale() {
    let mut app0 = b"JFIF\0".to_vec();
    app0.extend_from_slice(&[1, 2, 1]); // version 1.2, units=dpi
    app0.extend_from_slice(&300u16.to_be_bytes());
    app0.extend_from_slice(&300u16.to_be_bytes());
    app0.extend_from_slice(&[0, 0]); // no embedded thumbnail
    let data = jpeg(&[jpeg_segment(0xE0, &app0)]);
    let bundle = extract(&data, &MetadataLimits::default());
    // Pixel dimensions shrink drastically; density must not be touched or
    // flagged just because of that.
    let prepared = prepare(
        &bundle,
        &facts(10, 5, ColorAction::Unchanged),
        None,
        &MetadataLimits::default(),
    );
    assert!(prepared.report().issues.is_empty(), "{:?}", prepared.report());
}

#[test]
fn repeated_extended_xmp_issue_is_deduplicated() {
    // Two `rdf:Description` elements each declaring the ExtendedXMP
    // reference: `xmp::correct` removes each occurrence and raises one
    // issue per occurrence, so `prepare` must collapse the two identical
    // (category, reason, field) issues into one.
    let xml = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
      xmlns:xmpNote="http://ns.adobe.com/xmp/note/">
      <rdf:Description xmpNote:HasExtendedXMP="GUID1"/>
      <rdf:Description xmpNote:HasExtendedXMP="GUID2"/>
    </rdf:RDF>"#;
    let data = jpeg(&[xmp_segment(xml)]);
    let bundle = extract(&data, &MetadataLimits::default());
    let prepared = prepare(
        &bundle,
        &facts(100, 50, ColorAction::Unchanged),
        None,
        &MetadataLimits::default(),
    );
    let matching: Vec<_> = prepared
        .report()
        .issues
        .iter()
        .filter(|i| {
            i.category == MetadataCategory::Xmp
                && i.reason == MetadataIssueReason::Unsupported
                && i.field.as_deref() == Some("extended_xmp")
        })
        .collect();
    assert_eq!(matching.len(), 1, "{:?}", prepared.report());
}
