use criterion::{criterion_group, criterion_main, Criterion};

use r3sizer_core::{
    classifier, color, chroma_guard,
    metrics::{channel_clipping_ratio, pixel_out_of_gamut_ratio},
    sharpen,
    ClassificationParams, LinearRgbImage, ChromaRegionFactors,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn synthetic_image(w: u32, h: u32) -> LinearRgbImage {
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let r = x as f32 / (w - 1) as f32;
            let g = y as f32 / (h - 1) as f32;
            let b = ((x + y) % 2) as f32;
            data.extend_from_slice(&[r, g, b]);
        }
    }
    LinearRgbImage::new(w, h, data).unwrap()
}

fn synthetic_luma(w: usize, h: usize) -> Vec<f32> {
    (0..w * h)
        .map(|i| {
            let x = i % w;
            let y = i / w;
            x as f32 / (w - 1).max(1) as f32 * 0.5
                + y as f32 / (h - 1).max(1) as f32 * 0.5
        })
        .collect()
}

const W: u32 = 960;
const H: u32 = 540;

// ---------------------------------------------------------------------------
// Blur benchmarks
// ---------------------------------------------------------------------------

fn bench_blur(c: &mut Criterion) {
    let mut group = c.benchmark_group("blur");

    let kernel = sharpen::make_kernel(1.0).unwrap();
    let luma = synthetic_luma(W as usize, H as usize);
    let img = synthetic_image(W, H);

    group.bench_function("detail_single_channel_540p", |b| {
        b.iter(|| {
            sharpen::compute_detail_single_channel(&luma, W as usize, H as usize, &kernel);
        });
    });

    group.bench_function("detail_rgb_540p", |b| {
        b.iter(|| {
            sharpen::compute_detail_rgb(&img, &kernel);
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Color benchmarks
// ---------------------------------------------------------------------------

fn bench_color(c: &mut Criterion) {
    let mut group = c.benchmark_group("color");

    let img = synthetic_image(W, H);
    let luma = color::extract_luminance(&img);

    group.bench_function("extract_luminance_540p", |b| {
        b.iter(|| {
            color::extract_luminance(&img);
        });
    });

    group.bench_function("reconstruct_rgb_from_lightness_540p", |b| {
        b.iter(|| {
            color::reconstruct_rgb_from_lightness(&img, &luma);
        });
    });

    group.bench_function("image_srgb_to_linear_540p", |b| {
        b.iter_batched(
            || img.clone(),
            |mut clone| color::image_srgb_to_linear(&mut clone),
            criterion::BatchSize::LargeInput,
        );
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Classifier benchmark
// ---------------------------------------------------------------------------

fn bench_classifier(c: &mut Criterion) {
    let mut group = c.benchmark_group("classifier");

    let img = synthetic_image(W, H);
    let params = ClassificationParams::default();

    group.bench_function("classify_540p", |b| {
        b.iter(|| {
            classifier::classify(&img, &params);
        });
    });

    // Benchmark with pre-extracted luminance (avoids redundant extraction).
    let luma = color::extract_luminance(&img);
    group.bench_function("classify_with_luminance_540p", |b| {
        b.iter(|| {
            classifier::classify_with_luminance(&luma, W, H, &params);
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Metrics benchmarks
// ---------------------------------------------------------------------------

fn bench_metrics(c: &mut Criterion) {
    let mut group = c.benchmark_group("metrics");

    let img = synthetic_image(W, H);

    group.bench_function("channel_clipping_ratio_540p", |b| {
        b.iter(|| {
            channel_clipping_ratio(&img);
        });
    });

    group.bench_function("pixel_out_of_gamut_ratio_540p", |b| {
        b.iter(|| {
            pixel_out_of_gamut_ratio(&img);
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Sharpen apply (the cheap per-probe multiply-add)
// ---------------------------------------------------------------------------

fn bench_sharpen_apply(c: &mut Criterion) {
    let mut group = c.benchmark_group("sharpen_apply");

    let luma = synthetic_luma(W as usize, H as usize);
    let kernel = sharpen::make_kernel(1.0).unwrap();
    let detail = sharpen::compute_detail_single_channel(
        &luma,
        W as usize,
        H as usize,
        &kernel,
    );

    group.bench_function("apply_detail_single_channel_540p", |b| {
        b.iter(|| {
            sharpen::apply_detail_single_channel(&luma, &detail, 1.5);
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Chroma guard benchmark
// ---------------------------------------------------------------------------

fn bench_chroma_guard(c: &mut Criterion) {
    let mut group = c.benchmark_group("chroma_guard");

    let img = synthetic_image(W, H);
    let kernel = sharpen::make_kernel(1.0).unwrap();
    let detail_rgb = sharpen::compute_detail_rgb(&img, &kernel);
    let sharpened = sharpen::apply_detail_rgb(&img, &detail_rgb, 1.5);

    let region_map = classifier::classify(&img, &ClassificationParams::default());
    let region_factors = ChromaRegionFactors::default();

    group.bench_function("apply_chroma_guard_540p", |b| {
        b.iter(|| {
            chroma_guard::apply_chroma_guard(
                &img,
                &sharpened,
                0.05,
                Some(&region_map),
                Some(&region_factors),
                None,
            )
            .unwrap();
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Probe-loop benchmark (detail + N probes with metric)
// ---------------------------------------------------------------------------

fn bench_probe_loop(c: &mut Criterion) {
    let mut group = c.benchmark_group("probe_loop");

    let img = synthetic_image(W, H);
    let luma = color::extract_luminance(&img);
    let kernel = sharpen::make_kernel(1.0).unwrap();
    let detail = sharpen::compute_detail_single_channel(
        &luma, W as usize, H as usize, &kernel,
    );
    let strengths: Vec<f32> = vec![0.1, 0.3, 0.5, 0.8, 1.0, 1.3, 1.6, 2.0, 2.5, 3.0];

    // Full probe loop: apply detail + reconstruct RGB + metric per probe
    group.bench_function("10_probes_lightness_540p", |b| {
        b.iter(|| {
            let w = W as usize;
            let h = H as usize;
            let mut luma_scratch = vec![0.0f32; w * h];
            let mut rgb_scratch = LinearRgbImage::zeros(W, H).unwrap();
            let mut results = Vec::with_capacity(strengths.len());
            for &s in &strengths {
                sharpen::apply_detail_single_channel_into(
                    &luma, &detail, s, &mut luma_scratch,
                );
                color::reconstruct_rgb_from_lightness_into(
                    &img, &luma_scratch, &luma, &mut rgb_scratch,
                );
                let ratio = channel_clipping_ratio(&rgb_scratch);
                results.push((s, ratio));
            }
            results
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

criterion_group!(
    benches,
    bench_blur,
    bench_color,
    bench_classifier,
    bench_metrics,
    bench_sharpen_apply,
    bench_chroma_guard,
    bench_probe_loop,
);
criterion_main!(benches);
