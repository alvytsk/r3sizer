//! Integration tests for metadata-aware native I/O (`r3sizer_io::metadata`).
//!
//! Uses the binary fixtures committed for `r3sizer-metadata`'s own Task 5
//! tests (`metadata.jpg`/`plain.jpg`/etc, plus small synthetic JPEGs built
//! byte-by-byte here, the same way `r3sizer-metadata`'s own test suite
//! does) rather than adding new binary fixtures to this crate.

use std::path::{Path, PathBuf};

use r3sizer_io::{load_with_metadata, save_with_metadata, DecodeLimits};
use r3sizer_metadata::{MetadataIssueReason, MetadataLimits};

const METADATA_JPG: &[u8] = include_bytes!("../../r3sizer-metadata/tests/fixtures/metadata.jpg");
const PLAIN_JPG: &[u8] = include_bytes!("../../r3sizer-metadata/tests/fixtures/plain.jpg");
const PLAIN_PNG: &[u8] = include_bytes!("../../r3sizer-metadata/tests/fixtures/plain.png");

// ---------------------------------------------------------------------------
// Synthetic-JPEG helpers (byte-level construction, no external fixtures --
// mirrors r3sizer-metadata's own `#[cfg(test)]` helpers such as
// `exif::mod::tests::tiny_exif`).
// ---------------------------------------------------------------------------

/// Build a minimal little-endian TIFF IFD0 (as it appears after the
/// `Exif\0\0` prefix in a JPEG APP1 segment) from `(tag, type, count,
/// inline value)` entries. Only SHORT (type 3, count 1) and LONG (type 4,
/// count 1) inline values are supported -- enough for ImageWidth/
/// ImageLength/Orientation.
fn build_tiff(entries: &[(u16, u16, u32, u32)]) -> Vec<u8> {
    let mut b = b"II\x2a\0\x08\0\0\0".to_vec();
    b.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for &(tag, kind, count, value) in entries {
        b.extend_from_slice(&tag.to_le_bytes());
        b.extend_from_slice(&kind.to_le_bytes());
        b.extend_from_slice(&count.to_le_bytes());
        if kind == 3 && count == 1 {
            let mut field = [0u8; 4];
            field[0..2].copy_from_slice(&(value as u16).to_le_bytes());
            b.extend_from_slice(&field);
        } else {
            b.extend_from_slice(&value.to_le_bytes());
        }
    }
    b.extend_from_slice(&0u32.to_le_bytes()); // next IFD offset
    b
}

/// Wrap `tiff` bytes into a JPEG APP1 Exif marker segment (`FF E1`, a
/// big-endian length counting itself, then `Exif\0\0` + `tiff`).
fn build_exif_app1(tiff: &[u8]) -> Vec<u8> {
    let mut payload = b"Exif\0\0".to_vec();
    payload.extend_from_slice(tiff);
    let len = (payload.len() + 2) as u16;
    let mut seg = vec![0xFF, 0xE1];
    seg.extend_from_slice(&len.to_be_bytes());
    seg.extend_from_slice(&payload);
    seg
}

/// Wrap a 132-byte structurally-valid ICC profile into a single-segment
/// JPEG APP2 `ICC_PROFILE` marker segment. Mirrors the structural shape
/// `r3sizer_metadata::policy`'s own tests build (declared size = byte
/// count, `acsp` signature at the fixed offset, a zero-entry tag table).
fn build_icc_app2() -> Vec<u8> {
    let mut icc = vec![7u8; 132];
    let icc_len = icc.len() as u32;
    icc[0..4].copy_from_slice(&icc_len.to_be_bytes());
    icc[36..40].copy_from_slice(b"acsp");
    icc[128..132].copy_from_slice(&0u32.to_be_bytes());

    let mut payload = b"ICC_PROFILE\0".to_vec();
    payload.push(1); // sequence number
    payload.push(1); // sequence count
    payload.extend_from_slice(&icc);
    let len = (payload.len() + 2) as u16;
    let mut seg = vec![0xFF, 0xE2];
    seg.extend_from_slice(&len.to_be_bytes());
    seg.extend_from_slice(&payload);
    seg
}

/// Insert `segment` right after `base`'s leading APP0/JFIF segment (JFIF
/// must stay the first marker after SOI). `base` is assumed to start with
/// `FF D8 FF E0` (SOI then APP0), as `plain.jpg` does.
fn insert_after_app0(base: &[u8], segment: &[u8]) -> Vec<u8> {
    assert_eq!(&base[0..4], &[0xFF, 0xD8, 0xFF, 0xE0], "expected SOI+APP0 prefix");
    let app0_len = u16::from_be_bytes([base[4], base[5]]) as usize;
    let insert_at = 4 + app0_len;
    let mut out = Vec::with_capacity(base.len() + segment.len());
    out.extend_from_slice(&base[..insert_at]);
    out.extend_from_slice(segment);
    out.extend_from_slice(&base[insert_at..]);
    out
}

fn write_bytes(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn read_exif(path: &Path) -> exif::Exif {
    exif::Reader::new()
        .read_from_container(&mut std::io::BufReader::new(std::fs::File::open(path).unwrap()))
        .unwrap()
}

// ---------------------------------------------------------------------------
// Step 1
// ---------------------------------------------------------------------------

/// `metadata.jpg` (Task 5's fixture) has no EXIF `Orientation` tag at all
/// (verified independently against the real bytes with `kamadak-exif`), so
/// this compares `Option`s rather than unconditionally unwrapping -- it
/// still fails if a real orientation value were ever dropped or altered.
/// The `Artist` tag *is* present, so that's asserted directly, matching
/// what the test's name promises.
#[test]
fn native_export_preserves_source_orientation_and_artist() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("input.jpg");
    let dest = dir.path().join("output.png");
    std::fs::write(&source, METADATA_JPG).unwrap();
    let limits = MetadataLimits::default();

    let loaded = load_with_metadata(&source, &DecodeLimits::default(), &limits).unwrap();
    let expected = read_exif(&source);

    save_with_metadata(&loaded.image, &dest, &loaded, &limits).unwrap();
    let actual = read_exif(&dest);

    let orientation = |exif: &exif::Exif| {
        exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
            .map(|f| f.value.get_uint(0))
    };
    assert_eq!(orientation(&actual), orientation(&expected));

    let artist = |exif: &exif::Exif| {
        exif.get_field(exif::Tag::Artist, exif::In::PRIMARY)
            .unwrap()
            .display_value()
            .to_string()
    };
    assert_eq!(artist(&actual), artist(&expected));
}

// ---------------------------------------------------------------------------
// Step 4 matrix
// ---------------------------------------------------------------------------

#[test]
fn pixel_parity_with_load_as_linear() {
    let dir = tempfile::tempdir().unwrap();
    let source = write_bytes(dir.path(), "in.jpg", METADATA_JPG);

    let plain = r3sizer_io::load_as_linear(&source).unwrap();
    let with_meta = load_with_metadata(&source, &DecodeLimits::default(), &MetadataLimits::default())
        .unwrap();

    assert_eq!(plain.width(), with_meta.image.width());
    assert_eq!(plain.height(), with_meta.image.height());
    assert_eq!(plain.pixels(), with_meta.image.pixels());
}

#[test]
fn default_dimension_limits_reject_oversized_image() {
    let dir = tempfile::tempdir().unwrap();
    let source = write_bytes(dir.path(), "in.png", PLAIN_PNG);

    let tiny_limits = DecodeLimits {
        max_dimension: 3,
        max_pixels: 1_000_000,
    };
    let err = load_with_metadata(&source, &tiny_limits, &MetadataLimits::default())
        .expect_err("32x16 image should exceed a 3px dimension cap");
    assert!(matches!(err, r3sizer_io::IoError::TooLarge { .. }), "got {err}");
}

#[test]
fn reduced_metadata_cap_marks_metadata_unavailable_but_pixels_still_load() {
    let dir = tempfile::tempdir().unwrap();
    let source = write_bytes(dir.path(), "in.jpg", METADATA_JPG);

    let tiny_cap = MetadataLimits {
        max_source_bytes: 10,
        ..MetadataLimits::default()
    };
    let loaded = load_with_metadata(&source, &DecodeLimits::default(), &tiny_cap).unwrap();

    assert!(loaded.image.width() > 0 && loaded.image.height() > 0);
    assert!(loaded
        .metadata
        .report()
        .issues
        .iter()
        .any(|i| i.reason == MetadataIssueReason::LimitExceeded));
}

#[test]
fn no_metadata_round_trips_without_warnings() {
    let dir = tempfile::tempdir().unwrap();
    let source = write_bytes(dir.path(), "in.png", PLAIN_PNG);
    let dest = dir.path().join("out.png");
    let limits = MetadataLimits::default();

    let loaded = load_with_metadata(&source, &DecodeLimits::default(), &limits).unwrap();
    assert!(loaded.metadata.report().issues.is_empty());

    let report = save_with_metadata(&loaded.image, &dest, &loaded, &limits).unwrap();
    assert!(report.issues.is_empty(), "unexpected issues: {:?}", report.issues);
}

#[test]
fn malformed_metadata_with_valid_pixels_reports_issue_but_still_exports() {
    let dir = tempfile::tempdir().unwrap();

    // Valid TIFF, then corrupt the byte-order marker so the embedded EXIF
    // block is structurally invalid while the JPEG's own pixel data (which
    // this corruption never touches) stays intact.
    let mut tiff = build_tiff(&[(0x0100, 4, 1, 32), (0x0101, 4, 1, 16), (0x0112, 3, 1, 6)]);
    tiff[0] = b'X';
    tiff[1] = b'X';
    let jpg = insert_after_app0(PLAIN_JPG, &build_exif_app1(&tiff));
    let source = write_bytes(dir.path(), "in.jpg", &jpg);
    let dest = dir.path().join("out.png");
    let limits = MetadataLimits::default();

    let loaded = load_with_metadata(&source, &DecodeLimits::default(), &limits).unwrap();
    // extract() only captures the raw EXIF bytes; it doesn't parse the TIFF
    // structure, so no issue is raised yet at load time.
    assert_eq!(loaded.image.width(), 32);
    assert_eq!(loaded.image.height(), 16);

    let report = save_with_metadata(&loaded.image, &dest, &loaded, &limits).unwrap();
    assert!(
        report.issues.iter().any(|i| i.reason == MetadataIssueReason::Malformed),
        "expected a Malformed issue, got {:?}",
        report.issues
    );
    // The export itself still succeeds and produces a valid image.
    let out = image::open(&dest).unwrap();
    assert_eq!((out.width(), out.height()), (32, 16));
}

#[test]
fn unsupported_bmp_destination_warns_when_source_has_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let source = write_bytes(dir.path(), "in.jpg", METADATA_JPG);
    let dest = dir.path().join("out.bmp");
    let limits = MetadataLimits::default();

    let loaded = load_with_metadata(&source, &DecodeLimits::default(), &limits).unwrap();

    let report = save_with_metadata(&loaded.image, &dest, &loaded, &limits).unwrap();
    assert!(
        report.issues.iter().any(|i| i.reason == MetadataIssueReason::MergeFailed),
        "expected MergeFailed issues for an unsupported destination, got {:?}",
        report.issues
    );
    let out = image::open(&dest).unwrap();
    assert_eq!((out.width(), out.height()), (32, 16));
}

#[test]
fn unsupported_tiff_destination_with_no_source_metadata_has_no_issues() {
    let dir = tempfile::tempdir().unwrap();
    // `plain.png` (unlike `plain.jpg`, which always carries a JFIF density
    // payload) has genuinely no metadata: the PNG encoder emits no pHYs
    // chunk by default.
    let source = write_bytes(dir.path(), "in.png", PLAIN_PNG);
    let dest = dir.path().join("out.tiff");
    let limits = MetadataLimits::default();

    let loaded = load_with_metadata(&source, &DecodeLimits::default(), &limits).unwrap();
    let report = save_with_metadata(&loaded.image, &dest, &loaded, &limits).unwrap();
    assert!(
        report.issues.is_empty(),
        "an unsupported destination with no source metadata to carry should not warn: {:?}",
        report.issues
    );
}

#[test]
fn input_equals_output_preserves_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_bytes(dir.path(), "roundtrip.jpg", METADATA_JPG);
    let limits = MetadataLimits::default();

    // Capture pixels + metadata fully before any write touches `path`.
    let loaded = load_with_metadata(&path, &DecodeLimits::default(), &limits).unwrap();
    let expected_artist = read_exif(&path)
        .get_field(exif::Tag::Artist, exif::In::PRIMARY)
        .unwrap()
        .display_value()
        .to_string();

    // Save back over the same path.
    save_with_metadata(&loaded.image, &path, &loaded, &limits).unwrap();

    let actual_artist = read_exif(&path)
        .get_field(exif::Tag::Artist, exif::In::PRIMARY)
        .unwrap()
        .display_value()
        .to_string();
    assert_eq!(actual_artist, expected_artist);
}

#[test]
fn save_with_metadata_reports_disk_write_failure_not_success() {
    let dir = tempfile::tempdir().unwrap();
    let source = write_bytes(dir.path(), "in.png", PLAIN_PNG);
    // Parent directory does not exist -> the final `fs::write` must fail.
    let dest = dir.path().join("missing_subdir").join("out.png");
    let limits = MetadataLimits::default();

    let loaded = load_with_metadata(&source, &DecodeLimits::default(), &limits).unwrap();
    let err = save_with_metadata(&loaded.image, &dest, &loaded, &limits)
        .expect_err("write to a missing directory must fail, not succeed");
    assert!(matches!(err, r3sizer_io::IoError::Io(_)), "got {err}");
    assert!(!dest.exists());
}

#[test]
fn native_orientation_values_round_trip_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let limits = MetadataLimits::default();

    for orientation in 1u32..=8 {
        let tiff = build_tiff(&[
            (0x0100, 4, 1, 32),
            (0x0101, 4, 1, 16),
            (0x0112, 3, 1, orientation),
        ]);
        let jpg = insert_after_app0(PLAIN_JPG, &build_exif_app1(&tiff));
        let source = write_bytes(dir.path(), &format!("in_{orientation}.jpg"), &jpg);
        let dest = dir.path().join(format!("out_{orientation}.jpg"));

        let loaded = load_with_metadata(&source, &DecodeLimits::default(), &limits).unwrap();
        assert_eq!(loaded.orientation, r3sizer_metadata::OrientationAction::Preserve);

        save_with_metadata(&loaded.image, &dest, &loaded, &limits).unwrap();
        let actual = read_exif(&dest)
            .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
            .unwrap()
            .value
            .get_uint(0);
        assert_eq!(actual, Some(orientation));
    }
}

#[test]
fn icc_warning_for_unverified_source_color() {
    let dir = tempfile::tempdir().unwrap();
    let jpg = insert_after_app0(PLAIN_JPG, &build_icc_app2());
    let source = write_bytes(dir.path(), "in.jpg", &jpg);
    let dest = dir.path().join("out.jpg");
    let limits = MetadataLimits::default();

    let loaded = load_with_metadata(&source, &DecodeLimits::default(), &limits).unwrap();
    assert_eq!(loaded.color, r3sizer_metadata::ColorAction::Unverified);

    let report = save_with_metadata(&loaded.image, &dest, &loaded, &limits).unwrap();
    assert!(
        report.issues.iter().any(|i| {
            i.category == r3sizer_metadata::MetadataCategory::Icc
                && i.reason == MetadataIssueReason::Unverified
        }),
        "expected an ICC Unverified warning, got {:?}",
        report.issues
    );
}
