/// Load an image file and convert it to a linear-RGB `LinearRgbImage`.
///
/// Supported formats: any format supported by the `image` crate (PNG, JPEG,
/// BMP, TIFF, WebP, …).
///
/// Pipeline:
///   1. Decode file via `image::open`.
///   2. Convert to `Rgb8` (8-bit, 3 channels).
///   3. Normalise bytes to `f32` in [0, 1] (sRGB encoded).
///   4. Apply sRGB → linear transfer function (IEC 61966-2-1).
use std::path::Path;

use image::GenericImageView;
use imgsharp_core::{color, LinearRgbImage};

use crate::{convert::u8_rgb_to_linear_image, IoError};

/// Load a raster image from `path` and return it as a linear-RGB f32 image.
///
/// The image is decoded, converted to 8-bit RGB, normalised, and linearised.
/// The resulting `LinearRgbImage` is ready to be passed directly to
/// `process_auto_sharp_downscale`.
pub fn load_as_linear(path: &Path) -> Result<LinearRgbImage, IoError> {
    let dyn_img = image::open(path)?;
    let (width, height) = dyn_img.dimensions();

    // Convert to 8-bit RGB (drops alpha; any input depth is quantised here).
    let rgb8 = dyn_img.into_rgb8();
    let bytes: Vec<u8> = rgb8.into_raw();

    let mut img = u8_rgb_to_linear_image(width, height, bytes)?;

    // Apply sRGB → linear transfer function.
    color::image_srgb_to_linear(&mut img);

    Ok(img)
}
