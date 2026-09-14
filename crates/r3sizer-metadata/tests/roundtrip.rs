//! Cross-format merge/roundtrip tests: exercise the public `merge()` API
//! against real encoder output (`tests/fixtures/*`, see
//! `tests/fixtures/README.md`), not synthetic byte sequences -- Task 5's
//! job is making a real third-party encoder's JPEG/PNG/WebP survive
//! `img-parts` container mutation, which the hand-built fixtures in
//! `tests/containers.rs` don't exercise.
//!
//! Byte sequences built directly in this file (JPEG/PNG/WebP segment and
//! chunk helpers) are deliberately duplicated from `tests/containers.rs`'s
//! own local helpers rather than imported, matching that file's existing
//! style of keeping every test fixture's byte layout visible and
//! independent of production code.

use r3sizer_metadata::*;

const PLAIN_JPG: &[u8] = include_bytes!("fixtures/plain.jpg");
const PLAIN_PNG: &[u8] = include_bytes!("fixtures/plain.png");
const PLAIN_WEBP: &[u8] = include_bytes!("fixtures/plain.webp");
const METADATA_JPG: &[u8] = include_bytes!("fixtures/metadata.jpg");
const METADATA_PNG: &[u8] = include_bytes!("fixtures/metadata.png");
const METADATA_WEBP: &[u8] = include_bytes!("fixtures/metadata.webp");
const EXPECTED_JSON: &str = include_str!("fixtures/expected.json");

/// The single source of truth for every injected fixture value and its
/// byte offset within the shared TIFF block -- see
/// `tests/fixtures/expected.json` and `tests/gen_fixtures.rs`'s
/// `TiffOffsets`.
fn expected() -> serde_json::Value {
    serde_json::from_str(EXPECTED_JSON).expect("expected.json must parse")
}

fn expected_str(field: &str) -> String {
    expected()[field].as_str().unwrap_or_else(|| panic!("expected.json missing string field {field}")).to_string()
}

fn expected_offset(field: &str) -> usize {
    expected()["tiff_value_offsets"][field]
        .as_u64()
        .unwrap_or_else(|| panic!("expected.json missing tiff_value_offsets.{field}")) as usize
}

fn find(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .position(|w| w == needle)
        .unwrap_or_else(|| panic!("needle not found"))
}

fn facts() -> OutputFacts {
    OutputFacts {
        width: 32,
        height: 16,
        orientation: OrientationAction::Normalize,
        color: ColorAction::Srgb,
    }
}

fn facts_with(color: ColorAction) -> OutputFacts {
    OutputFacts { color, ..facts() }
}

// --- Byte-building helpers (independent duplicates of tests/containers.rs) -

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

fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32fast::hash(&out[4..]);
    out.extend_from_slice(&crc.to_be_bytes());
    out
}

fn png_ihdr() -> Vec<u8> {
    png_chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0])
}

fn png(chunks: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(png_ihdr());
    for c in chunks {
        out.extend_from_slice(c);
    }
    out.extend(png_chunk(b"IEND", &[]));
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
    out.extend(body);
    out
}

/// A structurally valid (but not necessarily sRGB-equivalent) minimal ICC
/// profile: 128-byte header (declared size + `acsp` signature) followed by
/// a zero-entry tag table, matching `policy.rs`'s own test helper shape.
fn valid_icc(byte: u8) -> Vec<u8> {
    valid_icc_sized(byte, 132)
}

/// Same shape as `valid_icc`, but padded to an arbitrary total length --
/// used to exercise the JPEG multi-segment ICC splitting path near its
/// real size boundary. `total_len` must be at least 132 (header + a
/// zero-entry tag table).
fn valid_icc_sized(byte: u8, total_len: usize) -> Vec<u8> {
    assert!(total_len >= 132);
    let mut b = vec![byte; total_len];
    let size = (b.len() as u32).to_be_bytes();
    b[0..4].copy_from_slice(&size);
    b[36..40].copy_from_slice(b"acsp");
    b[128..132].copy_from_slice(&0u32.to_be_bytes());
    b
}

fn irb(id: u16, data: &[u8]) -> Vec<u8> {
    let mut out = b"8BIM".to_vec();
    out.extend_from_slice(&id.to_be_bytes());
    out.push(0); // zero-length Pascal name
    out.push(0); // pad
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(data);
    if !data.len().is_multiple_of(2) {
        out.push(0);
    }
    out
}

fn iptc_dataset(record: u8, dataset: u8, value: &[u8]) -> Vec<u8> {
    let mut d = vec![0x1Cu8, record, dataset];
    d.extend_from_slice(&(value.len() as u16).to_be_bytes());
    d.extend_from_slice(value);
    d
}

fn read_artist(bytes: &[u8]) -> Option<String> {
    let tags = exif::Reader::new()
        .read_from_container(&mut std::io::Cursor::new(bytes))
        .ok()?;
    tags.get_field(exif::Tag::Artist, exif::In::PRIMARY)
        .map(|f| f.display_value().to_string())
}

// --- Step 2 test (brief-specified) -----------------------------------------

#[test]
fn preserves_artist_across_all_supported_destinations() {
    let limits = MetadataLimits::default();
    let sources: [&[u8]; 3] = [METADATA_JPG, METADATA_PNG, METADATA_WEBP];
    let destinations: [&[u8]; 3] = [PLAIN_JPG, PLAIN_PNG, PLAIN_WEBP];
    for source in sources {
        let bundle = extract(source, &limits);
        for dest in destinations {
            let output = merge(dest.to_vec(), &bundle, &facts(), &limits);
            assert!(read_artist(&output.bytes).unwrap().contains(&expected_str("artist")));
            assert_eq!(
                image::load_from_memory(&output.bytes).unwrap().to_rgba8(),
                image::load_from_memory(dest).unwrap().to_rgba8()
            );
        }
    }
}

// --- expected.json value-offset map is real, and matches the fixtures -----

/// `expected.json`'s `tiff_value_offsets` map is asserted here against the
/// *committed fixture bytes directly* (not through this crate's own
/// extraction), proving each documented offset really does locate that
/// value's raw content within the shared TIFF block -- the same block
/// embedded, byte-for-byte, as the JPEG APP1 payload (after `Exif\0\0`),
/// the PNG `eXIf` chunk data, and the WebP `EXIF` chunk data.
#[test]
fn expected_json_offsets_locate_the_real_artist_and_copyright_bytes() {
    let artist = expected_str("artist").into_bytes();
    let copyright = expected_str("copyright").into_bytes();
    let artist_offset = expected_offset("artist");
    let copyright_offset = expected_offset("copyright");

    let jpeg_tiff_start = find(METADATA_JPG, b"Exif\0\0") + 6;
    assert_eq!(
        &METADATA_JPG[jpeg_tiff_start + artist_offset..][..artist.len()],
        artist.as_slice()
    );
    assert_eq!(
        &METADATA_JPG[jpeg_tiff_start + copyright_offset..][..copyright.len()],
        copyright.as_slice()
    );

    // PNG's eXIf chunk data (length(4) + "eXIf"(4) + data) is the TIFF
    // block directly, no "Exif\0\0" prefix.
    let png_tiff_start = find(METADATA_PNG, b"eXIf") + 4;
    assert_eq!(
        &METADATA_PNG[png_tiff_start + artist_offset..][..artist.len()],
        artist.as_slice()
    );

    // WebP's EXIF chunk data (fourcc(4) + size(4) + data) is likewise the
    // TIFF block directly.
    let webp_tiff_start = find(METADATA_WEBP, b"EXIF") + 8;
    assert_eq!(
        &METADATA_WEBP[webp_tiff_start + artist_offset..][..artist.len()],
        artist.as_slice()
    );
}

// --- No metadata source -----------------------------------------------------

#[test]
fn no_metadata_source_leaves_same_format_destination_byte_identical() {
    let limits = MetadataLimits::default();
    for (source, dest) in [
        (PLAIN_JPG, PLAIN_JPG),
        (PLAIN_PNG, PLAIN_PNG),
        (PLAIN_WEBP, PLAIN_WEBP),
    ] {
        let bundle = extract(source, &limits);
        assert!(bundle.report().issues.is_empty(), "{:?}", bundle.report());
        let output = merge(dest.to_vec(), &bundle, &facts(), &limits);
        assert_eq!(output.bytes, dest, "untouched container must round-trip byte-for-byte");
    }
}

// --- Same-format IPTC / comment / text / density ---------------------------

#[test]
fn same_format_iptc_survives_jpeg_to_jpeg() {
    let limits = MetadataLimits::default();
    let dataset = iptc_dataset(2, 5, b"hello iptc");
    let mut app13 = b"Photoshop 3.0\0".to_vec();
    app13.extend(irb(0x0404, &dataset));
    let source = jpeg(&[jpeg_segment(0xED, &app13)]);
    let bundle = extract(&source, &limits);
    assert!(bundle.report().issues.is_empty(), "{:?}", bundle.report());

    let output = merge(PLAIN_JPG.to_vec(), &bundle, &facts(), &limits);
    assert!(
        !output.report.issues.iter().any(|i| i.reason == MetadataIssueReason::MergeFailed),
        "{:?}",
        output.report
    );
    // Re-extract the merged bytes and confirm the IPTC dataset round-tripped.
    let merged_bundle = extract(&output.bytes, &limits);
    assert!(!merged_bundle.report().issues.iter().any(|i| i.category == MetadataCategory::Iptc));
    image::load_from_memory(&output.bytes).expect("still a valid JPEG");
}

#[test]
fn same_format_comment_survives_jpeg_to_jpeg() {
    let limits = MetadataLimits::default();
    let source = jpeg(&[jpeg_segment(0xFE, b"a fixture comment")]);
    let bundle = extract(&source, &limits);
    assert!(bundle.report().issues.is_empty());

    let output = merge(PLAIN_JPG.to_vec(), &bundle, &facts(), &limits);
    assert!(bytes_contain(&output.bytes, b"a fixture comment"));
}

#[test]
fn same_format_text_survives_png_to_png() {
    let limits = MetadataLimits::default();
    let source = png(&[png_chunk(b"tEXt", b"Description\0Fixture PNG text")]);
    let bundle = extract(&source, &limits);
    assert!(bundle.report().issues.is_empty());

    let output = merge(PLAIN_PNG.to_vec(), &bundle, &facts(), &limits);
    assert!(bytes_contain(&output.bytes, b"Fixture PNG text"));
    image::load_from_memory(&output.bytes).expect("still a valid PNG");
}

#[test]
fn same_format_density_survives_jpeg_to_jpeg() {
    let limits = MetadataLimits::default();
    let mut app0 = b"JFIF\0".to_vec();
    app0.extend_from_slice(&[1, 2, 1]); // version, units=dpi
    app0.extend_from_slice(&300u16.to_be_bytes());
    app0.extend_from_slice(&300u16.to_be_bytes());
    app0.extend_from_slice(&[0, 0]);
    let source = jpeg(&[jpeg_segment(0xE0, &app0)]);
    let bundle = extract(&source, &limits);
    assert!(bundle.report().issues.is_empty());

    let output = merge(PLAIN_JPG.to_vec(), &bundle, &facts(), &limits);
    let merged_bundle = extract(&output.bytes, &limits);
    assert!(merged_bundle.report().issues.is_empty(), "{:?}", merged_bundle.report());
    assert!(bytes_contain(&output.bytes, &300u16.to_be_bytes()));
}

#[test]
fn same_format_density_survives_png_to_png_even_when_destination_has_no_phys() {
    let limits = MetadataLimits::default();
    let source = png(&[png_chunk(b"pHYs", &[0, 0, 0x0B, 0x13, 0, 0, 0x0B, 0x13, 1])]); // ~2835 ppm = 72dpi
    let bundle = extract(&source, &limits);
    assert!(bundle.report().issues.is_empty());

    let output = merge(PLAIN_PNG.to_vec(), &bundle, &facts(), &limits);
    assert!(
        !output.report.issues.iter().any(|i| i.category == MetadataCategory::Density),
        "{:?}",
        output.report
    );
    image::load_from_memory(&output.bytes).expect("still a valid PNG");
}

fn bytes_contain(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

// --- Unsupported cross-format categories ------------------------------------

#[test]
fn iptc_and_comment_into_png_and_webp_are_unsupported_not_silently_dropped() {
    let limits = MetadataLimits::default();
    let dataset = iptc_dataset(2, 5, b"hello");
    let mut app13 = b"Photoshop 3.0\0".to_vec();
    app13.extend(irb(0x0404, &dataset));
    let mut segments = vec![jpeg_segment(0xED, &app13)];
    segments.push(jpeg_segment(0xFE, b"a comment"));
    let source = jpeg(&segments);
    let bundle = extract(&source, &limits);
    assert!(bundle.report().issues.is_empty(), "{:?}", bundle.report());

    for dest in [PLAIN_PNG, PLAIN_WEBP] {
        let output = merge(dest.to_vec(), &bundle, &facts(), &limits);
        assert!(
            output
                .report
                .issues
                .iter()
                .any(|i| i.category == MetadataCategory::Iptc && i.reason == MetadataIssueReason::Unsupported),
            "{:?}",
            output.report
        );
        assert!(
            output
                .report
                .issues
                .iter()
                .any(|i| i.category == MetadataCategory::Text && i.reason == MetadataIssueReason::Unsupported),
            "{:?}",
            output.report
        );
        assert!(
            !output.report.issues.iter().any(|i| i.reason == MetadataIssueReason::MergeFailed),
            "an unsupported category must not fail the whole merge: {:?}",
            output.report
        );
    }
}

#[test]
fn png_text_into_jpeg_and_webp_is_unsupported() {
    let limits = MetadataLimits::default();
    let source = png(&[png_chunk(b"tEXt", b"Author\0Fixture Author")]);
    let bundle = extract(&source, &limits);
    assert!(bundle.report().issues.is_empty());

    for dest in [PLAIN_JPG, PLAIN_WEBP] {
        let output = merge(dest.to_vec(), &bundle, &facts(), &limits);
        assert!(output
            .report
            .issues
            .iter()
            .any(|i| i.category == MetadataCategory::Text && i.reason == MetadataIssueReason::Unsupported));
    }
}

// --- Destination-encoder color tags -----------------------------------------

#[test]
fn unchanged_color_policy_retains_any_structurally_valid_source_icc() {
    let limits = MetadataLimits::default();
    let icc = valid_icc(7);
    let source = jpeg(&[jpeg_segment(0xE2, &{
        let mut seg = b"ICC_PROFILE\0".to_vec();
        seg.extend_from_slice(&[1, 1]);
        seg.extend_from_slice(&icc);
        seg
    })]);
    let bundle = extract(&source, &limits);
    assert!(bundle.report().issues.is_empty(), "{:?}", bundle.report());

    let output = merge(PLAIN_JPG.to_vec(), &bundle, &facts_with(ColorAction::Unchanged), &limits);
    let merged_bundle = extract(&output.bytes, &limits);
    assert_eq!(merged_bundle.source_color(), SourceColor::Other);
}

#[test]
fn srgb_policy_drops_icc_when_destination_has_no_matching_profile() {
    let limits = MetadataLimits::default();
    let icc = valid_icc(7);
    let source = jpeg(&[jpeg_segment(0xE2, &{
        let mut seg = b"ICC_PROFILE\0".to_vec();
        seg.extend_from_slice(&[1, 1]);
        seg.extend_from_slice(&icc);
        seg
    })]);
    let bundle = extract(&source, &limits);

    let output = merge(PLAIN_JPG.to_vec(), &bundle, &facts_with(ColorAction::Srgb), &limits);
    assert!(output
        .report
        .issues
        .iter()
        .any(|i| i.category == MetadataCategory::Icc && i.reason == MetadataIssueReason::Unverified));
    let merged_bundle = extract(&output.bytes, &limits);
    assert_ne!(merged_bundle.source_color(), SourceColor::Other);
}

// --- Large ICC profiles: multi-segment splitting near the real boundary ----
//
// `containers::jpeg::set_icc`'s `ICC_SEGMENT_MAX_SIZE` must exactly match
// `img_parts::jpeg::image::ICC_SEGMENT_MAX_SIZE` (65,519); a profile chunk
// even one byte over that produces a `JpegSegment` whose content exceeds
// 65,533 bytes, which panics inside `img-parts`' own encoder
// (`(self.len() - 2).try_into::<u16>()`) rather than returning gracefully.
// These tests build the *source* ICC profile via a WebP `ICCP` chunk
// (uncompressed, no JPEG-segment-sized ceiling of its own) specifically so
// a profile of this size can exist as a `Payload::Icc` at all, then merge
// it into a real JPEG destination -- exercising `set_icc`'s splitting path
// for real, not just the constant in isolation.

/// Reassembles every `ICC_PROFILE\0`-prefixed APP2 segment's data (by
/// sequence number) directly via `img-parts` -- a dependency this crate
/// already has, used here as an independent check that `set_icc` produced
/// well-formed segments `img-parts` itself can still parse, not just that
/// nothing panicked.
fn reassemble_icc_via_img_parts(jpeg_bytes: &[u8]) -> Vec<u8> {
    let jpeg = img_parts::jpeg::Jpeg::from_bytes(img_parts::Bytes::from(jpeg_bytes.to_vec()))
        .expect("output must still be a parseable JPEG");
    let mut parts: Vec<(u8, u8, Vec<u8>)> = jpeg
        .segments()
        .iter()
        .filter(|s| s.marker() == 0xE2 && s.contents().starts_with(b"ICC_PROFILE\0"))
        .map(|s| {
            let c = s.contents();
            (c[12], c[13], c[14..].to_vec())
        })
        .collect();
    parts.sort_by_key(|p| p.0);
    parts.into_iter().flat_map(|p| p.2).collect()
}

#[test]
fn icc_profile_exactly_at_the_segment_boundary_splits_without_panicking() {
    let limits = MetadataLimits::default();
    // 65,519 bytes: exactly `ICC_SEGMENT_MAX_SIZE`. At the *old, buggy*
    // constant (65,521) this would have fit in what the code believed was
    // one segment while actually needing 65,533 (segment cap) + 2 more --
    // triggering the panic. At the correct constant, this size is safely
    // within (indeed exactly at) one segment's payload capacity.
    let icc = valid_icc_sized(3, 65_519);
    let source = webp(&[webp_chunk(b"ICCP", &icc)]);
    let bundle = extract(&source, &limits);
    assert!(bundle.report().issues.is_empty(), "{:?}", bundle.report());

    let output = merge(PLAIN_JPG.to_vec(), &bundle, &facts_with(ColorAction::Unchanged), &limits); // must not panic
    assert!(
        !output.report.issues.iter().any(|i| i.reason == MetadataIssueReason::MergeFailed),
        "{:?}",
        output.report
    );
    assert_eq!(reassemble_icc_via_img_parts(&output.bytes), icc);
}

#[test]
fn icc_profile_one_byte_past_the_segment_boundary_forces_a_second_segment() {
    let limits = MetadataLimits::default();
    // 65,520 bytes: one byte past `ICC_SEGMENT_MAX_SIZE`, genuinely forcing
    // a second (1-byte) segment. This is the exact size the reviewer's
    // reproduction used to trigger the pre-fix panic.
    let icc = valid_icc_sized(5, 65_520);
    let source = webp(&[webp_chunk(b"ICCP", &icc)]);
    let bundle = extract(&source, &limits);
    assert!(bundle.report().issues.is_empty(), "{:?}", bundle.report());

    let output = merge(PLAIN_JPG.to_vec(), &bundle, &facts_with(ColorAction::Unchanged), &limits); // must not panic
    assert!(
        !output.report.issues.iter().any(|i| i.reason == MetadataIssueReason::MergeFailed),
        "{:?}",
        output.report
    );
    let jpeg = img_parts::jpeg::Jpeg::from_bytes(img_parts::Bytes::from(output.bytes.clone())).unwrap();
    let icc_segments = jpeg
        .segments()
        .iter()
        .filter(|s| s.marker() == 0xE2 && s.contents().starts_with(b"ICC_PROFILE\0"))
        .count();
    assert!(icc_segments >= 2, "a profile past the boundary must span at least two segments, got {icc_segments}");
    assert_eq!(reassemble_icc_via_img_parts(&output.bytes), icc);
}

// --- JPEG segment-size limits ------------------------------------------------

/// A source whose *decompressed/parsed* EXIF is
/// large enough that, however it made it into the bundle, the destination
/// JPEG's APP1 payload cap (65,533 bytes, prefix included) is what must
/// reject it -- not this crate's much larger general `max_payload_bytes`.
#[test]
fn oversized_exif_via_real_source_is_limit_exceeded_and_never_embedded() {
    let limits = MetadataLimits {
        max_payload_bytes: 200_000, // let extraction/correction accept a big IFD
        max_total_metadata_bytes: 200_000,
        ..MetadataLimits::default()
    };

    // A single huge ASCII tag (Artist) whose value alone exceeds the
    // 65,533-byte JPEG APP1 cap once the "Exif\0\0" prefix is added.
    let value_len = 65_600usize;
    let mut tiff = b"II\x2a\0\x08\0\0\0".to_vec();
    tiff.extend(1u16.to_le_bytes());
    tiff.extend(0x013bu16.to_le_bytes()); // Artist
    tiff.extend(2u16.to_le_bytes()); // ASCII
    tiff.extend((value_len as u32).to_le_bytes());
    let value_offset = tiff.len() as u32 + 4 + 4; // after this entry's offset field + next-IFD field
    tiff.extend(value_offset.to_le_bytes());
    tiff.extend(0u32.to_le_bytes());
    tiff.extend(vec![b'a'; value_len]);

    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend_from_slice(&tiff);
    // This APP1 payload is itself over 65,533 bytes, so it can't be
    // expressed with a 16-bit JPEG segment length either -- confirming
    // that a *source* this large could only ever have gotten here via a
    // format without that ceiling (PNG eXIf, WebP EXIF, both plain RIFF
    // chunks/PNG chunks with a 32-bit length). Build the source as a PNG.
    let source = png(&[png_chunk(b"eXIf", &tiff)]);
    let bundle = extract(&source, &limits);
    assert!(bundle.report().issues.is_empty(), "{:?}", bundle.report());

    let output = merge(PLAIN_JPG.to_vec(), &bundle, &facts(), &limits);
    assert!(
        output
            .report
            .issues
            .iter()
            .any(|i| i.category == MetadataCategory::Exif && i.reason == MetadataIssueReason::LimitExceeded),
        "{:?}",
        output.report
    );
    assert!(read_artist(&output.bytes).is_none());
    image::load_from_memory(&output.bytes).expect("still a valid JPEG despite the omission");
}

// --- Opaque/unknown destination blocks are left alone -----------------------

#[test]
fn merge_never_strips_an_unrelated_destination_segment() {
    let limits = MetadataLimits::default();
    let marker_bytes = b"totally unrelated opaque payload";
    let mut dest = PLAIN_JPG.to_vec();
    // Splice an unrecognized APP4 segment right after SOI (a stand-in for
    // an encoder-specific declaration this crate has no opinion about).
    let segment = jpeg_segment(0xE4, marker_bytes);
    dest.splice(2..2, segment);

    let bundle = extract(METADATA_JPG, &limits);
    let output = merge(dest.clone(), &bundle, &facts(), &limits);
    assert!(bytes_contain(&output.bytes, marker_bytes));
    assert!(read_artist(&output.bytes).unwrap().contains(&expected_str("artist")));
}

// --- Corrupt metadata --------------------------------------------------------

#[test]
fn corrupt_source_metadata_yields_no_embedded_exif_but_a_valid_destination() {
    let limits = MetadataLimits::default();
    // Two conflicting EXIF APP1 segments: extraction drops EXIF entirely as
    // ambiguous (see `containers::jpeg`'s `duplicate_exif_is_dropped_as_conflicting`).
    let mut a = b"Exif\0\0".to_vec();
    a.extend_from_slice(b"MM\0*one");
    let mut b = b"Exif\0\0".to_vec();
    b.extend_from_slice(b"MM\0*two");
    let source = jpeg(&[jpeg_segment(0xE1, &a), jpeg_segment(0xE1, &b)]);
    let bundle = extract(&source, &limits);
    assert!(bundle
        .report()
        .issues
        .iter()
        .any(|i| i.category == MetadataCategory::Exif && i.reason == MetadataIssueReason::Malformed));

    let output = merge(PLAIN_JPG.to_vec(), &bundle, &facts(), &limits);
    assert!(read_artist(&output.bytes).is_none());
    assert!(output
        .report
        .issues
        .iter()
        .any(|i| i.category == MetadataCategory::Exif && i.reason == MetadataIssueReason::Malformed));
    image::load_from_memory(&output.bytes).expect("still a valid JPEG");
}

// --- Container rollback ------------------------------------------------------

#[test]
fn dimension_mismatch_rolls_back_to_byte_identical_original_with_merge_failed() {
    let limits = MetadataLimits::default();
    let bundle = extract(METADATA_JPG, &limits);
    // The real destination is 32x16; claim something else so this
    // adapter's own post-write dimension validation must refuse to return
    // the mutated bytes.
    let wrong_facts = OutputFacts {
        width: 999,
        height: 999,
        orientation: OrientationAction::Normalize,
        color: ColorAction::Srgb,
    };
    let output = merge(PLAIN_JPG.to_vec(), &bundle, &wrong_facts, &limits);
    assert_eq!(output.bytes, PLAIN_JPG, "rollback must return the ORIGINAL bytes unchanged");
    assert!(
        output
            .report
            .issues
            .iter()
            .any(|i| i.category == MetadataCategory::Exif && i.reason == MetadataIssueReason::MergeFailed),
        "every attempted category must be reported MergeFailed: {:?}",
        output.report
    );
}

#[test]
fn webp_dimension_mismatch_also_rolls_back_byte_identical() {
    let limits = MetadataLimits::default();
    let bundle = extract(METADATA_WEBP, &limits);
    let wrong_facts = OutputFacts {
        width: 1,
        height: 1,
        orientation: OrientationAction::Normalize,
        color: ColorAction::Srgb,
    };
    let output = merge(PLAIN_WEBP.to_vec(), &bundle, &wrong_facts, &limits);
    assert_eq!(output.bytes, PLAIN_WEBP);
    assert!(output
        .report
        .issues
        .iter()
        .any(|i| i.reason == MetadataIssueReason::MergeFailed));
}

// --- WebP VP8X/flags specifics -----------------------------------------------

/// A synthetic minimal VP8L chunk: only the 5-byte header is real (the
/// only part any scanner in this crate reads), the rest is an inert filler
/// that is never decoded as pixels by anything under test here.
fn synthetic_vp8l(width: u32, height: u32, alpha: bool) -> Vec<u8> {
    let w = (width - 1) & 0x3FFF;
    let h = (height - 1) & 0x3FFF;
    let alpha_bit: u32 = if alpha { 1 } else { 0 };
    let bits: u32 = w | (h << 14) | (alpha_bit << 28);
    let mut payload = vec![0x2fu8];
    payload.extend_from_slice(&bits.to_le_bytes());
    payload.extend_from_slice(&[0u8; 8]); // inert bitstream tail
    webp_chunk(b"VP8L", &payload)
}

fn synthetic_webp(width: u32, height: u32, alpha: bool) -> Vec<u8> {
    webp(&[synthetic_vp8l(width, height, alpha)])
}

fn first_chunk_id(bytes: &[u8]) -> [u8; 4] {
    [bytes[12], bytes[13], bytes[14], bytes[15]]
}

#[test]
fn xmp_only_insertion_into_plain_webp_produces_a_valid_extended_header() {
    let limits = MetadataLimits::default();
    let source = jpeg(&[jpeg_segment(0xE1, &{
        let mut app1 = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
        app1.extend_from_slice(b"<x:xmpmeta>fixture</x:xmpmeta>");
        app1
    })]);
    let bundle = extract(&source, &limits);
    assert!(bundle.report().issues.is_empty(), "{:?}", bundle.report());

    let dest = synthetic_webp(32, 16, false);
    let output = merge(dest, &bundle, &facts(), &limits);
    assert!(
        !output.report.issues.iter().any(|i| i.reason == MetadataIssueReason::MergeFailed),
        "{:?}",
        output.report
    );
    assert_eq!(first_chunk_id(&output.bytes), *b"VP8X");
    // VP8X payload starts right after its own 8-byte chunk header (fourcc+size).
    let flags = output.bytes[12 + 8];
    assert_eq!(flags, 0b0000_0100, "only the XMP bit should be set");
}

#[test]
fn adding_exif_to_an_existing_alpha_vp8l_file_keeps_the_alpha_flag() {
    let limits = MetadataLimits::default();
    let bundle = extract(METADATA_JPG, &limits);
    let dest = synthetic_webp(32, 16, true); // alpha bit set in the VP8L header, no VP8X

    let output = merge(dest, &bundle, &facts(), &limits);
    assert!(
        !output.report.issues.iter().any(|i| i.reason == MetadataIssueReason::MergeFailed),
        "{:?}",
        output.report
    );
    assert_eq!(first_chunk_id(&output.bytes), *b"VP8X");
    let flags = output.bytes[12 + 8];
    assert_eq!(flags, 0b0001_1000, "EXIF bit (0x08) and alpha bit (0x10) must both be set");
    assert!(bytes_contain(&output.bytes, b"VP8L"), "the original VP8L chunk must survive untouched");
    assert!(read_artist(&output.bytes).unwrap().contains(&expected_str("artist")));
}

#[test]
fn merge_never_writes_a_second_vp8x() {
    let limits = MetadataLimits::default();
    let bundle = extract(METADATA_JPG, &limits);
    // Destination already extended (has its own VP8X with the alpha bit).
    let vp8x_payload = {
        let mut p = vec![0b0001_0000u8, 0, 0, 0];
        p.extend_from_slice(&31u32.to_le_bytes()[..3]); // width-1
        p.extend_from_slice(&15u32.to_le_bytes()[..3]); // height-1
        p
    };
    let dest = webp(&[webp_chunk(b"VP8X", &vp8x_payload), synthetic_vp8l(32, 16, true)]);

    let output = merge(dest, &bundle, &facts(), &limits);
    let vp8x_count = count_chunks(&output.bytes, b"VP8X");
    assert_eq!(vp8x_count, 1, "never insert a second VP8X");
}

fn count_chunks(webp: &[u8], fourcc: &[u8; 4]) -> usize {
    let mut count = 0;
    let mut pos = 12usize;
    while pos + 8 <= webp.len() {
        let id = &webp[pos..pos + 4];
        let size = u32::from_le_bytes([webp[pos + 4], webp[pos + 5], webp[pos + 6], webp[pos + 7]]) as usize;
        if id == fourcc {
            count += 1;
        }
        let pad = size % 2;
        pos += 8 + size + pad;
    }
    count
}

// --- Minimal/degenerate destinations must never panic ----------------------
//
// `img_parts::jpeg::Jpeg::set_exif`/`set_icc_profile` and
// `img_parts::png::Png::set_icc_profile` each hardcode an unconditional
// `Vec::insert` at a fixed index (3 for JPEG, 1 for PNG) with no bounds
// check, and their respective `from_bytes` parsers are lenient enough to
// accept a destination with fewer segments/chunks than that index -- e.g. a
// bare `SOI`+`EOI` JPEG (zero segments) or a signature-only PNG (zero
// chunks). This crate's own `set_exif`/`set_icc` (jpeg.rs, png.rs) exist
// specifically to avoid calling those library methods directly; these tests
// are the regression guard for that fix -- merely completing without a
// panic is the primary assertion.

#[test]
fn minimal_jpeg_destination_with_no_segments_never_panics_on_exif_or_icc() {
    let limits = MetadataLimits::default();
    // Bare SOI immediately followed by EOI: zero segments, well under the
    // index (3) `Jpeg::set_exif`/`set_icc_profile` insert at unconditionally.
    let bare = vec![0xFFu8, 0xD8, 0xFF, 0xD9];
    let bundle = extract(METADATA_JPG, &limits); // carries both EXIF and (via ICC test path) nothing else here
    let output = merge(bare.clone(), &bundle, &facts(), &limits); // must not panic
    // No SOF marker exists to prove the claimed 32x16 facts, so this
    // adapter's own dimension validation correctly refuses to return the
    // mutated bytes -- a graceful `MergeFailed` rollback, not a crash.
    assert_eq!(output.bytes, bare, "rollback must return the original bytes unchanged");
    assert!(output
        .report
        .issues
        .iter()
        .any(|i| i.reason == MetadataIssueReason::MergeFailed));

    // Same destination, this time with an ICC payload as the only content,
    // exercising `set_icc`'s own insert path.
    let icc_source = jpeg(&[jpeg_segment(0xE2, &{
        let mut seg = b"ICC_PROFILE\0".to_vec();
        seg.extend_from_slice(&[1, 1]);
        seg.extend_from_slice(&valid_icc(9));
        seg
    })]);
    let icc_bundle = extract(&icc_source, &limits);
    let icc_output = merge(bare.clone(), &icc_bundle, &facts_with(ColorAction::Unchanged), &limits); // must not panic
    assert_eq!(icc_output.bytes, bare);
}

#[test]
fn signature_only_png_destination_never_panics_on_icc() {
    let limits = MetadataLimits::default();
    // Just the 8-byte PNG signature: zero chunks, under the index (1)
    // `Png::set_icc_profile` inserts at unconditionally.
    let bare = b"\x89PNG\r\n\x1a\n".to_vec();
    let source = png(&[png_chunk(b"iCCP", &{
        let mut d = b"kw\0\0".to_vec();
        d.extend_from_slice(&flate2_compress(&valid_icc(9)));
        d
    })]);
    let bundle = extract(&source, &limits);
    assert!(bundle.report().issues.is_empty(), "{:?}", bundle.report());

    let output = merge(bare.clone(), &bundle, &facts_with(ColorAction::Unchanged), &limits); // must not panic
    assert_eq!(output.bytes, bare, "rollback must return the original bytes unchanged");
    assert!(output
        .report
        .issues
        .iter()
        .any(|i| i.reason == MetadataIssueReason::MergeFailed));
}

fn flate2_compress(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

// --- No tag values leak into issue text -------------------------------------

#[test]
fn issue_fields_never_contain_tag_values() {
    let limits = MetadataLimits::default();
    let mut tiff = b"II\x2a\0\x08\0\0\0".to_vec();
    tiff.extend(0u16.to_le_bytes());
    tiff.extend(0u32.to_le_bytes());
    let secret = b"top-secret-tag-value-should-never-leak";
    let mut app1 = b"Exif\0\0".to_vec();
    app1.extend_from_slice(&tiff);
    app1.extend_from_slice(secret);
    let source = jpeg(&[jpeg_segment(0xE1, &app1)]);
    let bundle = extract(&source, &limits);
    let output = merge(PLAIN_JPG.to_vec(), &bundle, &facts(), &limits);
    for issue in &output.report.issues {
        if let Some(field) = &issue.field {
            assert!(
                !field.as_bytes().windows(secret.len()).any(|w| w == secret),
                "issue field leaked a value: {field}"
            );
        }
    }
}
