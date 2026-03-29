//! Region-adaptive resize kernels (v0.4 experimental).
//!
//! Extends the resize stage to support multiple kernels and per-region
//! kernel selection based on the content classifier.
//!
//! Provenance: `EngineeringChoice` — paper-exact kernel unknown.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    classifier::classify,
    resize::downscale,
    types::{
        ClassificationParams, ImageSize, KernelTable, ResizeKernel,
        ResizeStrategy, ResizeStrategyDiagnostics,
    },
    CoreError, LinearRgbImage,
};

/// Map a [`ResizeKernel`] to the `image` crate's `FilterType`.
fn to_filter_type(kernel: ResizeKernel) -> image::imageops::FilterType {
    match kernel {
        ResizeKernel::Lanczos3 => image::imageops::FilterType::Lanczos3,
        ResizeKernel::MitchellNetravali => image::imageops::FilterType::CatmullRom, // Mitchell not in image crate; CatmullRom is closest cubic B-spline
        ResizeKernel::CatmullRom => image::imageops::FilterType::CatmullRom,
        ResizeKernel::Gaussian => image::imageops::FilterType::Gaussian,
    }
}

/// Downscale using a specific kernel.
pub fn downscale_with_kernel(
    src: &LinearRgbImage,
    target: ImageSize,
    kernel: ResizeKernel,
) -> Result<LinearRgbImage, CoreError> {
    if target.width == 0 || target.height == 0 {
        return Err(CoreError::InvalidParams("target dimensions must be non-zero".into()));
    }
    if src.width() == target.width && src.height() == target.height {
        return Ok(src.clone());
    }

    // For Lanczos3, delegate to existing path for consistency
    if kernel == ResizeKernel::Lanczos3 {
        return downscale(src, target);
    }

    let filter = to_filter_type(kernel);
    let buf: image::ImageBuffer<image::Rgb<f32>, Vec<f32>> =
        image::ImageBuffer::from_raw(src.width(), src.height(), src.pixels().to_vec())
            .expect("buffer length already validated by LinearRgbImage");

    let resized = image::imageops::resize(&buf, target.width, target.height, filter);
    let (w, h) = resized.dimensions();
    LinearRgbImage::new(w, h, resized.into_raw())
}

/// Content-adaptive resize: classify source, downscale with each needed kernel,
/// blend per-pixel according to region classification.
pub fn downscale_adaptive(
    src: &LinearRgbImage,
    target: ImageSize,
    classification: &ClassificationParams,
    kernel_table: &KernelTable,
) -> Result<(LinearRgbImage, ResizeStrategyDiagnostics), CoreError> {
    // 1. Classify the source image
    let region_map = classify(src, classification);

    // 2. Determine which distinct kernels are needed
    let all_kernels: Vec<ResizeKernel> = [
        kernel_table.flat,
        kernel_table.textured,
        kernel_table.strong_edge,
        kernel_table.microtexture,
        kernel_table.risky_halo_zone,
    ].into_iter().collect::<BTreeSet<_>>().into_iter().collect();

    // 3. If all regions map to the same kernel, fast path
    if all_kernels.len() == 1 {
        let kernel = all_kernels[0];
        let result = downscale_with_kernel(src, target, kernel)?;
        let total = target.width * target.height;
        let mut counts = BTreeMap::new();
        counts.insert(format!("{kernel:?}"), total);
        let diag = ResizeStrategyDiagnostics {
            kernels_used: vec![kernel],
            per_kernel_pixel_count: counts,
        };
        return Ok((result, diag));
    }

    // 4. Downscale with each needed kernel
    let mut kernel_results: BTreeMap<ResizeKernel, LinearRgbImage> = BTreeMap::new();
    for &kernel in &all_kernels {
        kernel_results.insert(kernel, downscale_with_kernel(src, target, kernel)?);
    }

    // 5. Build output by selecting per-pixel from the appropriate kernel result.
    //    Map each target pixel back to source coords (nearest-neighbor) to look up region class.
    let tw = target.width as usize;
    let th = target.height as usize;
    let sw = src.width() as f32;
    let sh = src.height() as f32;

    let mut output_data = vec![0.0f32; tw * th * 3];
    let mut per_kernel_count: BTreeMap<String, u32> = BTreeMap::new();
    for kernel in &all_kernels {
        per_kernel_count.insert(format!("{kernel:?}"), 0);
    }

    for y in 0..th {
        for x in 0..tw {
            // Map target pixel to source coordinates (nearest neighbor)
            let sx = ((x as f32 + 0.5) * sw / target.width as f32).min(sw - 1.0) as u32;
            let sy = ((y as f32 + 0.5) * sh / target.height as f32).min(sh - 1.0) as u32;
            let region = region_map.get(sx, sy);
            let kernel = kernel_table.kernel_for(region);

            let src_img = &kernel_results[&kernel];
            let src_data = src_img.pixels();
            let idx = (y * tw + x) * 3;
            output_data[idx] = src_data[idx];
            output_data[idx + 1] = src_data[idx + 1];
            output_data[idx + 2] = src_data[idx + 2];

            *per_kernel_count.entry(format!("{kernel:?}")).or_insert(0) += 1;
        }
    }

    let result = LinearRgbImage::new(target.width, target.height, output_data)?;
    let diag = ResizeStrategyDiagnostics {
        kernels_used: all_kernels,
        per_kernel_pixel_count: per_kernel_count,
    };
    Ok((result, diag))
}

/// Dispatch resize based on the configured strategy.
pub fn downscale_with_strategy(
    src: &LinearRgbImage,
    target: ImageSize,
    strategy: &ResizeStrategy,
) -> Result<(LinearRgbImage, ResizeStrategyDiagnostics), CoreError> {
    match strategy {
        ResizeStrategy::Uniform { kernel } => {
            let result = downscale_with_kernel(src, target, *kernel)?;
            let total = target.width * target.height;
            let mut counts = BTreeMap::new();
            counts.insert(format!("{kernel:?}"), total);
            let diag = ResizeStrategyDiagnostics {
                kernels_used: vec![*kernel],
                per_kernel_pixel_count: counts,
            };
            Ok((result, diag))
        }
        ResizeStrategy::ContentAdaptive { classification, kernel_table } => {
            downscale_adaptive(src, target, classification, kernel_table)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient_image(w: u32, h: u32) -> LinearRgbImage {
        let mut data = vec![0.0f32; (w * h * 3) as usize];
        for y in 0..h {
            for x in 0..w {
                let idx = ((y * w + x) * 3) as usize;
                data[idx] = x as f32 / w.max(1) as f32;
                data[idx + 1] = y as f32 / h.max(1) as f32;
                data[idx + 2] = 0.5;
            }
        }
        LinearRgbImage::new(w, h, data).unwrap()
    }

    #[test]
    fn downscale_with_kernel_lanczos3_matches_default() {
        let src = gradient_image(16, 16);
        let target = ImageSize { width: 4, height: 4 };
        let a = downscale(&src, target).unwrap();
        let b = downscale_with_kernel(&src, target, ResizeKernel::Lanczos3).unwrap();
        assert_eq!(a.pixels(), b.pixels());
    }

    #[test]
    fn downscale_with_kernel_all_variants_valid() {
        let src = gradient_image(16, 16);
        let target = ImageSize { width: 4, height: 4 };
        for kernel in [
            ResizeKernel::Lanczos3,
            ResizeKernel::MitchellNetravali,
            ResizeKernel::CatmullRom,
            ResizeKernel::Gaussian,
        ] {
            let result = downscale_with_kernel(&src, target, kernel).unwrap();
            assert_eq!(result.width(), 4);
            assert_eq!(result.height(), 4);
            assert!(result.pixels().iter().all(|v| v.is_finite()));
        }
    }

    #[test]
    fn different_kernels_produce_different_results() {
        let src = gradient_image(32, 32);
        let target = ImageSize { width: 8, height: 8 };
        let lanczos = downscale_with_kernel(&src, target, ResizeKernel::Lanczos3).unwrap();
        let gaussian = downscale_with_kernel(&src, target, ResizeKernel::Gaussian).unwrap();
        // They should differ
        let diff: f32 = lanczos.pixels().iter().zip(gaussian.pixels().iter())
            .map(|(a, b)| (a - b).abs())
            .sum();
        assert!(diff > 0.0);
    }

    #[test]
    fn adaptive_resize_produces_valid_output() {
        let src = gradient_image(32, 32);
        let target = ImageSize { width: 8, height: 8 };
        let (result, diag) = downscale_adaptive(
            &src,
            target,
            &ClassificationParams::default(),
            &KernelTable::default(),
        ).unwrap();
        assert_eq!(result.width(), 8);
        assert_eq!(result.height(), 8);
        assert!(!diag.kernels_used.is_empty());
        let total_pixels: u32 = diag.per_kernel_pixel_count.values().sum();
        assert_eq!(total_pixels, 64);
    }

    #[test]
    fn uniform_strategy_dispatch() {
        let src = gradient_image(16, 16);
        let target = ImageSize { width: 4, height: 4 };
        let (result, diag) = downscale_with_strategy(
            &src,
            target,
            &ResizeStrategy::Uniform { kernel: ResizeKernel::CatmullRom },
        ).unwrap();
        assert_eq!(result.width(), 4);
        assert_eq!(diag.kernels_used, vec![ResizeKernel::CatmullRom]);
    }
}
