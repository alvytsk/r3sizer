//! Fixture generator for `tests/roundtrip.rs`.
//!
//! This is not part of the normal test run: it is `#[ignore]`d and is the
//! executable form of the recipe documented in `tests/fixtures/README.md`.
//! Run it explicitly to (re)create the committed fixtures:
//!
//! ```sh
//! cargo test -p r3sizer-metadata --test gen_fixtures -- --ignored
//! ```
//!
//! Every byte here is produced independently of this crate's own extraction/
//! merge code: pixels come straight from the `image` crate's encoders, and
//! metadata is spliced in with hand-written byte code matching the JPEG/PNG/
//! WebP/TIFF specifications directly (mirroring the style already used by
//! this crate's own `#[cfg(test)]` fixture helpers in
//! `src/containers/{jpeg,png,webp}.rs`), not by calling `extract`/`merge`.

use std::path::Path;

const WIDTH: u32 = 32;
const HEIGHT: u32 = 16;

fn pixels() -> image::RgbImage {
    image::RgbImage::from_fn(WIDTH, HEIGHT, |x, y| {
        image::Rgb([
            if x < 16 { 240 } else { 20 },
            if y < 8 { 180 } else { 40 },
            90,
        ])
    })
}

fn encode(format: image::ImageFormat) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    pixels().write_to(&mut out, format).unwrap();
    out.into_inner()
}

// --- Hand-built TIFF/EXIF block (independent of src/exif) ------------------

fn ifd_entry(tag: u16, kind: u16, count: u32, value_field: [u8; 4]) -> Vec<u8> {
    let mut e = Vec::with_capacity(12);
    e.extend(tag.to_le_bytes());
    e.extend(kind.to_le_bytes());
    e.extend(count.to_le_bytes());
    e.extend(value_field);
    e
}

fn offset_field(offset: u32) -> [u8; 4] {
    offset.to_le_bytes()
}

fn inline_ascii(s: &[u8]) -> [u8; 4] {
    assert!(s.len() <= 4);
    let mut f = [0u8; 4];
    f[..s.len()].copy_from_slice(s);
    f
}

const TYPE_ASCII: u16 = 2;
const TYPE_LONG: u16 = 4;
const TYPE_RATIONAL: u16 = 5;

/// Builds a little-endian classic TIFF buffer (starting at the `II*\0`
/// byte-order marker, as `Payload::Exif` bytes are shaped) carrying:
/// `Artist` = "Fixture Author", `Copyright` = "Fixture Copyright",
/// `ImageDescription` = "Metadata fixture", an `ExifIFD` with
/// `DateTimeOriginal` = "2024:01:02 03:04:05", and a `GPSIFD` for
/// 1 deg 2 min 3 sec N, 4 deg 5 min 6 sec E.
fn build_tiff() -> Vec<u8> {
    let desc = b"Metadata fixture\0";
    let artist = b"Fixture Author\0";
    let copyright = b"Fixture Copyright\0";
    let datetime = b"2024:01:02 03:04:05\0";
    let lat_ref = b"N\0";
    let lon_ref = b"E\0";
    let lat_rationals: [u32; 6] = [1, 1, 2, 1, 3, 1];
    let lon_rationals: [u32; 6] = [4, 1, 5, 1, 6, 1];

    const IFD0_ENTRIES: u32 = 5;
    const EXIF_ENTRIES: u32 = 1;
    const GPS_ENTRIES: u32 = 4;
    let ifd0_size = 2 + IFD0_ENTRIES * 12 + 4;
    let exif_size = 2 + EXIF_ENTRIES * 12 + 4;
    let gps_size = 2 + GPS_ENTRIES * 12 + 4;

    let ifd0_offset: u32 = 8;
    let exif_offset = ifd0_offset + ifd0_size;
    let gps_offset = exif_offset + exif_size;
    let pool_offset = gps_offset + gps_size;

    let desc_offset = pool_offset;
    let artist_offset = desc_offset + desc.len() as u32;
    let copyright_offset = artist_offset + artist.len() as u32;
    let datetime_offset = copyright_offset + copyright.len() as u32;
    let lat_offset = datetime_offset + datetime.len() as u32;
    let lon_offset = lat_offset + 24;

    let mut b = b"II\x2a\0".to_vec();
    b.extend(ifd0_offset.to_le_bytes());

    let mut ifd0 = Vec::new();
    ifd0.extend((IFD0_ENTRIES as u16).to_le_bytes());
    ifd0.extend(ifd_entry(0x010e, TYPE_ASCII, desc.len() as u32, offset_field(desc_offset)));
    ifd0.extend(ifd_entry(0x013b, TYPE_ASCII, artist.len() as u32, offset_field(artist_offset)));
    ifd0.extend(ifd_entry(
        0x8298,
        TYPE_ASCII,
        copyright.len() as u32,
        offset_field(copyright_offset),
    ));
    ifd0.extend(ifd_entry(0x8769, TYPE_LONG, 1, offset_field(exif_offset)));
    ifd0.extend(ifd_entry(0x8825, TYPE_LONG, 1, offset_field(gps_offset)));
    ifd0.extend(0u32.to_le_bytes());

    let mut exif_ifd = Vec::new();
    exif_ifd.extend((EXIF_ENTRIES as u16).to_le_bytes());
    exif_ifd.extend(ifd_entry(
        0x9003,
        TYPE_ASCII,
        datetime.len() as u32,
        offset_field(datetime_offset),
    ));
    exif_ifd.extend(0u32.to_le_bytes());

    let mut gps_ifd = Vec::new();
    gps_ifd.extend((GPS_ENTRIES as u16).to_le_bytes());
    gps_ifd.extend(ifd_entry(0x0001, TYPE_ASCII, 2, inline_ascii(lat_ref)));
    gps_ifd.extend(ifd_entry(0x0002, TYPE_RATIONAL, 3, offset_field(lat_offset)));
    gps_ifd.extend(ifd_entry(0x0003, TYPE_ASCII, 2, inline_ascii(lon_ref)));
    gps_ifd.extend(ifd_entry(0x0004, TYPE_RATIONAL, 3, offset_field(lon_offset)));
    gps_ifd.extend(0u32.to_le_bytes());

    b.extend(ifd0);
    b.extend(exif_ifd);
    b.extend(gps_ifd);
    b.extend(desc);
    b.extend(artist);
    b.extend(copyright);
    b.extend(datetime);
    for v in lat_rationals {
        b.extend(v.to_le_bytes());
    }
    for v in lon_rationals {
        b.extend(v.to_le_bytes());
    }
    b
}

// --- JPEG splice -------------------------------------------------------

fn inject_jpeg_exif(jpg: &[u8], tiff: &[u8]) -> Vec<u8> {
    // Insert APP1 right after SOI, or after an existing APP0/JFIF segment
    // if present (JFIF must stay the first marker after SOI).
    let insert_at = if jpg.len() > 5 && jpg[2] == 0xFF && jpg[3] == 0xE0 {
        let seg_len = u16::from_be_bytes([jpg[4], jpg[5]]) as usize;
        4 + seg_len
    } else {
        2
    };
    let mut payload = b"Exif\0\0".to_vec();
    payload.extend_from_slice(tiff);
    let seg_len = (2 + payload.len()) as u16; // brief: 2 + 6 + tiff.len()
    let mut segment = vec![0xFF, 0xE1];
    segment.extend(seg_len.to_be_bytes());
    segment.extend(payload);

    let mut out = jpg[..insert_at].to_vec();
    out.extend(segment);
    out.extend(&jpg[insert_at..]);
    out
}

// --- PNG splice ----------------------------------------------------------

fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32fast::hash(&out[4..]);
    out.extend_from_slice(&crc.to_be_bytes());
    out
}

fn inject_png_exif(png: &[u8], tiff: &[u8]) -> Vec<u8> {
    // eXIf must precede the first IDAT (PNG spec Table 7).
    let idat_marker = png.windows(4).position(|w| w == b"IDAT").expect("IDAT present");
    let insert_at = idat_marker - 4; // back up over IDAT's own length field
    let mut out = png[..insert_at].to_vec();
    out.extend(png_chunk(b"eXIf", tiff));
    out.extend(&png[insert_at..]);
    out
}

// --- WebP splice -----------------------------------------------------------

fn webp_chunk(fourcc: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = fourcc.to_vec();
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    if !data.len().is_multiple_of(2) {
        out.push(0);
    }
    out
}

/// Converts a plain VP8/VP8L "simple" WebP into the "extended" (VP8X) form
/// carrying an EXIF chunk, rebuilding the RIFF size field per the WebP RFC
/// (RFC 9649): VP8X must be the first chunk, before the image data; EXIF
/// comes after it. This image has no alpha (plain RGB) and isn't animated,
/// so the flags byte only ever needs the EXIF bit (0x08).
fn inject_webp_exif(webp: &[u8], tiff: &[u8], width: u32, height: u32) -> Vec<u8> {
    assert_eq!(&webp[0..4], b"RIFF");
    assert_eq!(&webp[8..12], b"WEBP");
    let image_chunk = &webp[12..]; // the original VP8/VP8L chunk, already padded

    let mut vp8x_payload = vec![0x08u8, 0, 0, 0]; // flags (EXIF only), reserved
    vp8x_payload.extend_from_slice(&(width - 1).to_le_bytes()[..3]);
    vp8x_payload.extend_from_slice(&(height - 1).to_le_bytes()[..3]);
    let vp8x = webp_chunk(b"VP8X", &vp8x_payload);
    let exif_chunk = webp_chunk(b"EXIF", tiff);

    let mut body = b"WEBP".to_vec();
    body.extend(vp8x);
    body.extend_from_slice(image_chunk);
    body.extend(exif_chunk);

    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap();
}

#[test]
#[ignore = "run explicitly to (re)generate committed fixtures; see tests/fixtures/README.md"]
fn generate_fixtures() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    std::fs::create_dir_all(&dir).unwrap();

    let plain_jpg = encode(image::ImageFormat::Jpeg);
    let plain_png = encode(image::ImageFormat::Png);
    let plain_webp = encode(image::ImageFormat::WebP);
    write(&dir.join("plain.jpg"), &plain_jpg);
    write(&dir.join("plain.png"), &plain_png);
    write(&dir.join("plain.webp"), &plain_webp);

    let tiff = build_tiff();
    let metadata_jpg = inject_jpeg_exif(&plain_jpg, &tiff);
    let metadata_png = inject_png_exif(&plain_png, &tiff);
    let metadata_webp = inject_webp_exif(&plain_webp, &tiff, WIDTH, HEIGHT);
    write(&dir.join("metadata.jpg"), &metadata_jpg);
    write(&dir.join("metadata.png"), &metadata_png);
    write(&dir.join("metadata.webp"), &metadata_webp);

    // Independently verify with kamadak-exif before committing anything.
    for (name, bytes) in [
        ("metadata.jpg", &metadata_jpg),
        ("metadata.png", &metadata_png),
        ("metadata.webp", &metadata_webp),
    ] {
        let tags = exif::Reader::new()
            .read_from_container(&mut std::io::Cursor::new(bytes))
            .unwrap_or_else(|e| panic!("{name}: kamadak-exif failed to read: {e}"));
        let artist = tags
            .get_field(exif::Tag::Artist, exif::In::PRIMARY)
            .unwrap_or_else(|| panic!("{name}: missing Artist"));
        assert_eq!(artist.display_value().to_string(), "\"Fixture Author\"");
    }

    let expected = serde_json::json!({
        "width": WIDTH,
        "height": HEIGHT,
        "artist": "Fixture Author",
        "copyright": "Fixture Copyright",
        "image_description": "Metadata fixture",
        "date_time_original": "2024:01:02 03:04:05",
        "gps_latitude_ref": "N",
        "gps_latitude": "1 deg 2 min 3 sec",
        "gps_longitude_ref": "E",
        "gps_longitude": "4 deg 5 min 6 sec",
        "tiff_value_offsets": {
            "ifd0_offset": 8,
            "note": "offsets are relative to the start of the TIFF block (the Exif payload after 'Exif\\0\\0'); see build_tiff() in this file for the exact layout"
        }
    });
    write(
        &dir.join("expected.json"),
        serde_json::to_vec_pretty(&expected).unwrap().as_slice(),
    );

    println!("Fixtures written to {}", dir.display());
}
