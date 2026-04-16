use criterion::{criterion_group, criterion_main, Criterion};

use r3sizer_core::{
    metrics::channel_clipping_ratio, process_auto_sharp_downscale, sharpen::unsharp_mask,
    ArtifactMetric, AutoSharpParams, ClampPolicy, FitStrategy, LinearRgbImage, MetricMode,
    PipelineMode, ProbeConfig, SharpenMode,
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
        metric_mode: MetricMode::RelativeToBase,
        artifact_metric: ArtifactMetric::ChannelClippingRatio,
        ..Default::default()
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

fn bench_pipeline_modes(c: &mut Criterion) {
    let src = synthetic_image(1920, 1080);

    let mut group = c.benchmark_group("pipeline_mode_1080p_to_540p");
    for (name, mode) in [
        ("fast", PipelineMode::Fast),
        ("balanced", PipelineMode::Balanced),
        ("quality", PipelineMode::Quality),
    ] {
        let params = AutoSharpParams {
            pipeline_mode: Some(mode),
            ..AutoSharpParams::photo(960, 540)
        }
        .resolved();

        group.bench_function(name, |b| {
            b.iter(|| {
                process_auto_sharp_downscale(&src, &params).unwrap();
            });
        });
    }
    group.finish();
}

fn bench_staged_shrink(c: &mut Criterion) {
    // 6× shrink ratio — triggers staged shrink
    let src = synthetic_image(3840, 2160);
    let params = AutoSharpParams {
        pipeline_mode: Some(PipelineMode::Fast),
        ..AutoSharpParams::photo(640, 360)
    }
    .resolved();

    c.bench_function("pipeline_4k_to_360p_fast", |b| {
        b.iter(|| {
            process_auto_sharp_downscale(&src, &params).unwrap();
        });
    });
}

criterion_group!(
    benches,
    bench_full_pipeline,
    bench_sharpen_only,
    bench_channel_clipping_ratio,
    bench_pipeline_modes,
    bench_staged_shrink,
);
criterion_main!(benches);
