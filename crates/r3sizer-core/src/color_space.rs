//! RAW-friendly ingress — input color-space handling (v0.4 experimental).
//!
//! Allows the pipeline to accept pre-linearized or HDR data without
//! going through the standard sRGB→linear conversion in the IO layer.

use crate::{LinearRgbImage, CoreError};
use crate::types::InputColorSpace;
use crate::types::InputIngressDiagnostics;

/// Prepare input data according to the declared color space.
///
/// - `Srgb`: no-op (IO layer already linearized).
/// - `LinearRgb`: validates range, emits diagnostic if values exceed [0, 1].
/// - `RawLinear`: normalizes to [0, 1] by dividing by the global channel maximum.
pub fn prepare_input(
    input: &LinearRgbImage,
    color_space: InputColorSpace,
) -> Result<(LinearRgbImage, InputIngressDiagnostics), CoreError> {
    match color_space {
        InputColorSpace::Srgb => {
            let diag = InputIngressDiagnostics {
                declared_color_space: color_space,
                raw_value_min: None,
                raw_value_max: None,
                normalization_scale: None,
                out_of_range_fraction: None,
            };
            Ok((input.clone(), diag))
        }
        InputColorSpace::LinearRgb => {
            let data = input.pixels();
            let out_of_range = data.iter().filter(|&&v| !(0.0..=1.0).contains(&v)).count();
            let fraction = if data.is_empty() {
                0.0
            } else {
                out_of_range as f32 / data.len() as f32
            };
            let diag = InputIngressDiagnostics {
                declared_color_space: color_space,
                raw_value_min: None,
                raw_value_max: None,
                normalization_scale: None,
                out_of_range_fraction: Some(fraction),
            };
            Ok((input.clone(), diag))
        }
        InputColorSpace::RawLinear => {
            let data = input.pixels();
            if data.is_empty() {
                return Err(CoreError::EmptyImage);
            }
            let mut min_val = f32::INFINITY;
            let mut max_val = f32::NEG_INFINITY;
            for &v in data {
                if v < min_val { min_val = v; }
                if v > max_val { max_val = v; }
            }

            let (output, scale) = if max_val > 1.0 {
                let inv_max = 1.0 / max_val;
                let normalized: Vec<f32> = data.iter().map(|&v| v * inv_max).collect();
                let img = LinearRgbImage::new(input.width(), input.height(), normalized)?;
                (img, Some(max_val))
            } else {
                (input.clone(), None)
            };

            let diag = InputIngressDiagnostics {
                declared_color_space: color_space,
                raw_value_min: Some(min_val),
                raw_value_max: Some(max_val),
                normalization_scale: scale,
                out_of_range_fraction: None,
            };
            Ok((output, diag))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_image(width: u32, height: u32, data: Vec<f32>) -> LinearRgbImage {
        LinearRgbImage::new(width, height, data).unwrap()
    }

    #[test]
    fn srgb_is_noop() {
        let img = make_image(2, 1, vec![0.5, 0.3, 0.1, 0.9, 0.8, 0.7]);
        let (out, diag) = prepare_input(&img, InputColorSpace::Srgb).unwrap();
        assert_eq!(out.pixels(), img.pixels());
        assert!(diag.normalization_scale.is_none());
        assert!(diag.out_of_range_fraction.is_none());
    }

    #[test]
    fn linear_rgb_detects_out_of_range() {
        let img = make_image(2, 1, vec![0.5, 1.5, 0.1, -0.1, 0.8, 0.7]);
        let (out, diag) = prepare_input(&img, InputColorSpace::LinearRgb).unwrap();
        assert_eq!(out.pixels(), img.pixels());
        let frac = diag.out_of_range_fraction.unwrap();
        assert!(frac > 0.0);
        // 2 out of 6 values are out of range
        assert!((frac - 2.0 / 6.0).abs() < 1e-6);
    }

    #[test]
    fn raw_linear_normalizes() {
        let img = make_image(1, 1, vec![1.0, 2.0, 0.5]);
        let (out, diag) = prepare_input(&img, InputColorSpace::RawLinear).unwrap();
        let scale = diag.normalization_scale.unwrap();
        assert!((scale - 2.0).abs() < 1e-6);
        assert!((out.pixels()[0] - 0.5).abs() < 1e-6);
        assert!((out.pixels()[1] - 1.0).abs() < 1e-6);
        assert!((out.pixels()[2] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn raw_linear_no_normalization_needed() {
        let img = make_image(1, 1, vec![0.5, 0.3, 0.1]);
        let (out, diag) = prepare_input(&img, InputColorSpace::RawLinear).unwrap();
        assert!(diag.normalization_scale.is_none());
        assert_eq!(out.pixels(), img.pixels());
    }
}
