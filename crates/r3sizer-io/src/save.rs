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
    // Clone and convert to sRGB.
    let mut srgb = img.clone();
    color::image_linear_to_srgb(&mut srgb);

    // Convert to u8.
    let bytes = linear_image_to_u8_rgb(&srgb);

    // Build image buffer and save.
    let buf: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_raw(img.width(), img.height(), bytes).ok_or_else(|| {
            IoError::UnsupportedFormat("failed to build output ImageBuffer".into())
        })?;

    buf.save(path)?;
    Ok(())
}
