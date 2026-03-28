//! Gamut excursion metric (v0.1 baseline).
//!
//! Counts channel values or pixels outside [0, 1] in linear RGB.
//! This is the selection metric used by the solver in v0.2.

use crate::types::LinearRgbImage;

/// Per-channel clipping ratio. Denominator = W * H * 3.
pub fn channel_clipping_ratio(img: &LinearRgbImage) -> f32 {
    let pixels = img.pixels();
    let total = pixels.len();
    if total == 0 {
        return 0.0;
    }
    let out_of_range: u32 = pixels
        .iter()
        .map(|&v| (!(0.0..=1.0).contains(&v)) as u32)
        .sum();
    out_of_range as f32 / total as f32
}

/// Per-pixel out-of-gamut ratio. Denominator = W * H.
pub fn pixel_out_of_gamut_ratio(img: &LinearRgbImage) -> f32 {
    let pixels = img.pixels();
    let total_pixels = pixels.len() / 3;
    if total_pixels == 0 {
        return 0.0;
    }
    let oog: u32 = pixels
        .chunks_exact(3)
        .map(|rgb| rgb.iter().any(|&v| !(0.0..=1.0).contains(&v)) as u32)
        .sum();
    oog as f32 / total_pixels as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::LinearRgbImage;

    fn solid(width: u32, height: u32, value: f32) -> LinearRgbImage {
        let n = (width * height * 3) as usize;
        LinearRgbImage::new(width, height, vec![value; n]).unwrap()
    }

    #[test]
    fn all_zero_is_clean() {
        let img = solid(10, 10, 0.0);
        assert_eq!(channel_clipping_ratio(&img), 0.0);
    }

    #[test]
    fn all_one_is_clean() {
        let img = solid(10, 10, 1.0);
        assert_eq!(channel_clipping_ratio(&img), 0.0);
    }

    #[test]
    fn all_mid_is_clean() {
        let img = solid(10, 10, 0.5);
        assert_eq!(channel_clipping_ratio(&img), 0.0);
    }

    #[test]
    fn one_out_of_range_component() {
        let mut data = vec![0.5_f32; 12];
        data[0] = -0.001;
        let img = LinearRgbImage::new(2, 2, data).unwrap();
        let expected = 1.0 / 12.0;
        assert!((channel_clipping_ratio(&img) - expected).abs() < 1e-7);
    }

    #[test]
    fn all_components_above_one() {
        let img = solid(4, 4, 1.001);
        assert_eq!(channel_clipping_ratio(&img), 1.0);
    }

    #[test]
    fn all_components_below_zero() {
        let img = solid(4, 4, -0.5);
        assert_eq!(channel_clipping_ratio(&img), 1.0);
    }

    #[test]
    fn boundary_values_not_counted() {
        let mut data = vec![0.0_f32; 6];
        data[3] = 1.0;
        let img = LinearRgbImage::new(1, 2, data).unwrap();
        assert_eq!(channel_clipping_ratio(&img), 0.0);
    }

    #[test]
    fn pixel_all_zero_is_clean() {
        let img = solid(10, 10, 0.0);
        assert_eq!(pixel_out_of_gamut_ratio(&img), 0.0);
    }

    #[test]
    fn pixel_all_one_is_clean() {
        let img = solid(10, 10, 1.0);
        assert_eq!(pixel_out_of_gamut_ratio(&img), 0.0);
    }

    #[test]
    fn pixel_one_bad_channel_counts_one_pixel() {
        let mut data = vec![0.5_f32; 12];
        data[0] = -0.001;
        let img = LinearRgbImage::new(2, 2, data).unwrap();
        let expected = 1.0 / 4.0;
        assert!((pixel_out_of_gamut_ratio(&img) - expected).abs() < 1e-7);
    }

    #[test]
    fn pixel_all_three_bad_counts_one_pixel() {
        let mut data = vec![0.5_f32; 12];
        data[0] = 1.5;
        data[1] = -0.1;
        data[2] = 2.0;
        let img = LinearRgbImage::new(2, 2, data).unwrap();
        let expected = 1.0 / 4.0;
        assert!((pixel_out_of_gamut_ratio(&img) - expected).abs() < 1e-7);
    }

    #[test]
    fn pixel_ratio_leq_channel_ratio() {
        let mut data = vec![0.5_f32; 12];
        data[0] = 1.5;
        data[1] = -0.1;
        data[2] = 2.0;
        let img = LinearRgbImage::new(2, 2, data).unwrap();
        assert!(pixel_out_of_gamut_ratio(&img) <= channel_clipping_ratio(&img));
    }
}
