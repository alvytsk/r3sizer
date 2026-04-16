/// Load an image file and convert it to a linear-RGB `LinearRgbImage`.
///
/// Supported formats: any format supported by the `image` crate (PNG, JPEG,
/// BMP, TIFF, WebP, …).
///
/// Pipeline:
///   1. Decode file via `image::open`.
///   2. Convert to `Rgb8` (8-bit, 3 channels).
///   3. Normalise bytes to `f32` in \[0, 1\] (sRGB encoded).
///   4. Apply sRGB → linear transfer function (IEC 61966-2-1).
use std::path::Path;

use r3sizer_core::{color, LinearRgbImage};

use crate::{convert::u8_rgb_to_linear_image, IoError};

// ---------------------------------------------------------------------------
// Decode limits
// ---------------------------------------------------------------------------

/// Upper bounds applied before a full image decode to protect against
/// oversized or maliciously crafted inputs.
///
/// Passed to [`load_as_linear_with_limits`].  [`load_as_linear`] uses
/// [`DecodeLimits::default`], which caps at 100 MP and 16 384 px per axis.
#[derive(Debug, Clone)]
pub struct DecodeLimits {
    /// Maximum number of pixels (width × height).  Default: 100 000 000 (100 MP).
    pub max_pixels: u64,
    /// Maximum image dimension (width or height).  Default: 16 384 px.
    pub max_dimension: u32,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            max_pixels: 100_000_000,
            max_dimension: 16_384,
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Load a raster image from `path` and return it as a linear-RGB f32 image,
/// rejecting inputs that exceed [`DecodeLimits::default`].
///
/// This is a convenience wrapper around [`load_as_linear_with_limits`].
/// Call that function directly to customise the limits.
pub fn load_as_linear(path: &Path) -> Result<LinearRgbImage, IoError> {
    load_as_linear_with_limits(path, &DecodeLimits::default())
}

/// Load a raster image from `path`, enforcing the given size limits.
///
/// Reads the image header first to obtain width and height **before**
/// allocating the full pixel buffer.  If either dimension exceeds
/// `limits.max_dimension`, or if `width × height` exceeds `limits.max_pixels`,
/// returns [`IoError::TooLarge`] immediately.
///
/// On success, the resulting [`LinearRgbImage`] is ready to be passed to
/// [`process_auto_sharp_downscale`][r3sizer_core::process_auto_sharp_downscale].
pub fn load_as_linear_with_limits(
    path: &Path,
    limits: &DecodeLimits,
) -> Result<LinearRgbImage, IoError> {
    // --- Phase 1: lightweight header read to check dimensions ---
    let (width, height) = image::ImageReader::open(path)?
        .with_guessed_format()?
        .into_dimensions()?;

    if width > limits.max_dimension || height > limits.max_dimension {
        return Err(IoError::TooLarge { width, height });
    }
    let pixel_count = (width as u64) * (height as u64);
    if pixel_count > limits.max_pixels {
        return Err(IoError::TooLarge { width, height });
    }

    // --- Phase 2: full decode (dimensions are within budget) ---
    let dyn_img = image::open(path)?;
    let rgb8 = dyn_img.into_rgb8();
    let bytes: Vec<u8> = rgb8.into_raw();

    let mut img = u8_rgb_to_linear_image(width, height, bytes)?;
    color::image_srgb_to_linear(&mut img);

    Ok(img)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Write a minimal 4×4 RGB PNG to a temp file and return it.
    fn write_tiny_png() -> NamedTempFile {
        let mut f = NamedTempFile::with_suffix(".png").unwrap();
        // Create a 4×4 solid-color PNG using the `image` crate itself.
        let img = image::RgbImage::from_fn(4, 4, |_, _| image::Rgb([128u8, 64, 200]));
        let mut buf = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut buf),
            image::ImageFormat::Png,
        )
        .unwrap();
        f.write_all(&buf).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn tiny_png_loads_within_default_limits() {
        let f = write_tiny_png();
        let result = load_as_linear(f.path());
        assert!(result.is_ok(), "unexpected error: {:?}", result.err());
        let img = result.unwrap();
        assert_eq!(img.width(), 4);
        assert_eq!(img.height(), 4);
    }

    #[test]
    fn too_large_dimension_rejected() {
        let f = write_tiny_png();
        let limits = DecodeLimits {
            max_dimension: 3, // 4×4 image exceeds this
            max_pixels: 1_000_000,
        };
        let err = load_as_linear_with_limits(f.path(), &limits)
            .expect_err("should have been rejected");
        assert!(
            matches!(err, IoError::TooLarge { width: 4, height: 4 }),
            "expected TooLarge, got {err}"
        );
    }

    #[test]
    fn too_many_pixels_rejected() {
        let f = write_tiny_png();
        let limits = DecodeLimits {
            max_dimension: 65535,
            max_pixels: 15, // 4×4 = 16 pixels exceeds 15
        };
        let err = load_as_linear_with_limits(f.path(), &limits)
            .expect_err("should have been rejected");
        assert!(
            matches!(err, IoError::TooLarge { width: 4, height: 4 }),
            "expected TooLarge, got {err}"
        );
    }

    #[test]
    fn load_as_linear_uses_default_limits() {
        // load_as_linear wraps load_as_linear_with_limits(default) —
        // a 4×4 image is well within the 100 MP default cap.
        let f = write_tiny_png();
        assert!(load_as_linear(f.path()).is_ok());
    }
}
