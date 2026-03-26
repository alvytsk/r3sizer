//! Buffer layout helpers shared between load and save.

use imgsharp_core::LinearRgbImage;

use crate::IoError;

/// Convert a flat `Vec<u8>` in RGB order (3 bytes per pixel) to a
/// `LinearRgbImage` with values normalised to [0, 1].
///
/// The caller is responsible for applying the sRGB → linear conversion
/// afterwards (or beforehand).
pub fn u8_rgb_to_linear_image(
    width: u32,
    height: u32,
    bytes: Vec<u8>,
) -> Result<LinearRgbImage, IoError> {
    let expected = (width * height * 3) as usize;
    if bytes.len() != expected {
        return Err(IoError::UnsupportedFormat(format!(
            "expected {expected} bytes for {width}×{height} RGB image, got {}",
            bytes.len()
        )));
    }
    let floats: Vec<f32> = bytes.iter().map(|&b| b as f32 / 255.0).collect();
    Ok(LinearRgbImage::new(width, height, floats)?)
}

/// Convert a `LinearRgbImage` to a flat `Vec<u8>` in RGB order.
///
/// Values are clamped to [0, 1] before scaling to [0, 255] to prevent
/// wrapping artefacts.  The caller is responsible for converting from linear
/// to sRGB *before* calling this function.
pub fn linear_image_to_u8_rgb(img: &LinearRgbImage) -> Vec<u8> {
    img.pixels()
        .iter()
        .map(|&v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect()
}
