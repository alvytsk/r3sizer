use criterion::{criterion_group, criterion_main, Criterion};

use imgsharp_core::{
    metrics::channel_clipping_ratio,
    sharpen::unsharp_mask,
    ArtifactMetric, AutoSharpParams, ClampPolicy, FitStrategy, LinearRgbImage, MetricMode,
    ProbeConfig, SharpenMode, SharpenModel, process_auto_sharp_downscale,
};

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

fn bench_full_pipeline(c: &mut Criterion) {
    let src = synthetic_image(1920, 1080);
    let params = AutoSharpParams {
        target_width: 960,
        target_height: 540,
        probe_strengths: ProbeConfig::Explicit(vec![0.05, 0.1, 0.2, 0.4, 0.8, 1.5, 3.0]),
        target_artifact_ratio: 0.001,
        enable_contrast_leveling: false,
        sharpen_sigma: 1.0,
        fit_strategy: FitStrategy::Cubic,
        output_clamp: ClampPolicy::Clamp,
        sharpen_mode: SharpenMode::Lightness,
        sharpen_model: SharpenModel::PracticalUsm,
        metric_mode: MetricMode::RelativeToBase,
        artifact_metric: ArtifactMetric::ChannelClippingRatio,
    };

    c.bench_function("full_pipeline_1080p_to_540p", |b| {
        b.iter(|| {
            process_auto_sharp_downscale(&src, &params).unwrap();
        });
    });
}

fn bench_sharpen_only(c: &mut Criterion) {
    let img = synthetic_image(960, 540);
    c.bench_function("unsharp_mask_540p", |b| {
        b.iter(|| {
            unsharp_mask(&img, 1.5, 1.0).unwrap();
        });
    });
}

fn bench_channel_clipping_ratio(c: &mut Criterion) {
    let img = synthetic_image(960, 540);
    c.bench_function("channel_clipping_ratio_540p", |b| {
        b.iter(|| {
            channel_clipping_ratio(&img);
        });
    });
}

criterion_group!(benches, bench_full_pipeline, bench_sharpen_only, bench_channel_clipping_ratio);
criterion_main!(benches);
