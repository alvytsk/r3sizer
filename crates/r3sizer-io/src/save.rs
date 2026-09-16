/// Save a linear-RGB `LinearRgbImage` to a file.
///
/// Pipeline:
///   1. Convert linear → sRGB (in-place on a clone).
///   2. Scale [0, 1] float → [0, 255] u8 with clamping.
///   3. Encode and write via `image`.
///
/// Output format is inferred from the file extension (`.png`, `.jpg`, etc.).
use std::path::Path;

use image::{ImageBuffer, Rgb};
use r3sizer_core::{color, LinearRgbImage};

use crate::{convert::linear_image_to_u8_rgb, IoError};

/// Save `img` (in linear RGB) to `path`.
///
/// A clone of the pixel data is gamma-encoded before writing; the caller's
/// `LinearRgbImage` is not modified.
pub fn save_from_linear(img: &LinearRgbImage, path: &Path) -> Result<(), IoError> {
    let encoded = encode_linear_image(img, path)?;
    std::fs::write(path, &encoded)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared helpers (used by `crate::metadata` too)
// ---------------------------------------------------------------------------

/// Gamma-encode `img` and encode it to bytes in the format implied by
/// `path`'s extension, without writing to disk.
///
/// This is the pixel-encoding sequence factored out of [`save_from_linear`]:
/// clone, convert linear → sRGB, convert to u8, build an `ImageBuffer`,
/// encode. Encoding into an in-memory `Cursor<Vec<u8>>` (rather than
/// `ImageBuffer::save`, which opens the destination file itself) produces
/// byte-for-byte identical output for every format this crate enables --
/// `image`'s `save`/`save_buffer` path resolves to the same
/// `ImageFormat::from_path` + encoder-write sequence used here, just against
/// a `BufWriter<File>` instead of a `Cursor<Vec<u8>>` -- but leaves the
/// bytes in memory so callers that need them before writing (metadata
/// merge) don't have to read the file back.
pub(crate) fn encode_linear_image(img: &LinearRgbImage, path: &Path) -> Result<Vec<u8>, IoError> {
    // Clone and convert to sRGB.
    let mut srgb = img.clone();
    color::image_linear_to_srgb(&mut srgb);

    // Convert to u8.
    let bytes = linear_image_to_u8_rgb(&srgb);

    // Build image buffer.
    let buf: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_raw(img.width(), img.height(), bytes).ok_or_else(|| {
            IoError::UnsupportedFormat("failed to build output ImageBuffer".into())
        })?;

    let format = image::ImageFormat::from_path(path)?;
    let mut cursor = std::io::Cursor::new(Vec::new());
    buf.write_to(&mut cursor, format)?;
    Ok(cursor.into_inner())
}
