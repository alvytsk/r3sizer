//! Custom parameters example — demonstrates manual parameter construction.
//!
//! Usage:
//!   cargo run --example custom_params --manifest-path crates/r3sizer-io/Cargo.toml \
//!       -- input.jpg output.png 800 600
//!
//! Shows how to build `AutoSharpParams` by hand, choosing the sharpening
//! strategy, metric mode, and probe configuration explicitly.
use std::path::Path;

use r3sizer_core::{ClassificationParams, GainTable};
use r3sizer_core::prelude::*;
use r3sizer_io::{load_as_linear, save_from_linear};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 5 {
        eprintln!("Usage: custom_params <input> <output> <width> <height>");
        std::process::exit(1);
    }
    let width: u32 = args[3].parse()?;
    let height: u32 = args[4].parse()?;

    let src = load_as_linear(Path::new(&args[1]))?;

    // ------------------------------------------------------------------
    // Variant A: Uniform sharpening (no per-region gain map)
    //   - RGB sharpening mode (vs default Lightness)
    //   - Absolute artifact metric (vs default RelativeToBase)
    //   - Explicit probe strengths instead of TwoPass adaptive
    // ------------------------------------------------------------------
    let params_uniform = AutoSharpParams {
        target_width: width,
        target_height: height,
        // Explicit probe list — useful for reproducible benchmarking.
        probe_strengths: ProbeConfig::Explicit(vec![0.0, 0.1, 0.2, 0.4, 0.6, 0.8, 1.0]),
        target_artifact_ratio: 0.005,
        sharpen_mode: SharpenMode::Rgb,
        metric_mode: MetricMode::AbsoluteTotal,
        sharpen_strategy: SharpenStrategy::Uniform,
        // All other fields take Photo-preset defaults.
        ..AutoSharpParams::photo(width, height)
    }
    .resolved();

    let result_a = process_auto_sharp_downscale(&src, &params_uniform)?;
    let d = &result_a.diagnostics;
    println!(
        "[Uniform/RGB]    s* = {:.4}  mode = {:?}",
        d.selected_strength, d.selection_mode
    );
    save_from_linear(&result_a.image, Path::new(&args[2]))?;
    println!("Saved (uniform) → {:?}", &args[2]);

    // ------------------------------------------------------------------
    // Variant B: Content-adaptive sharpening (default strategy)
    //   - Lightness sharpening (paper-recommended)
    //   - RelativeToBase metric (only counts sharpening artifacts)
    //   - Two-pass adaptive probing (coarse scan → dense refinement)
    // ------------------------------------------------------------------
    let params_adaptive = AutoSharpParams {
        target_width: width,
        target_height: height,
        probe_strengths: ProbeConfig::TwoPass {
            coarse_count: 9,
            coarse_min: 0.002,
            coarse_max: 1.2,
            dense_count: 5,
            window_margin: 0.4,
        },
        target_artifact_ratio: 0.003,
        sharpen_mode: SharpenMode::Lightness,
        metric_mode: MetricMode::RelativeToBase,
        // ContentAdaptive is the default — shown explicitly for clarity.
        sharpen_strategy: SharpenStrategy::ContentAdaptive {
            classification: ClassificationParams::default(),
            gain_table: GainTable::v03_default(),
            max_backoff_iterations: 4,
            backoff_scale_factor: 0.8,
        },
        ..AutoSharpParams::photo(width, height)
    }
    .resolved();

    let result_b = process_auto_sharp_downscale(&src, &params_adaptive)?;
    let d = &result_b.diagnostics;
    println!(
        "[Adaptive/Luma]  s* = {:.4}  mode = {:?}",
        d.selected_strength, d.selection_mode
    );
    if let Some(cov) = &d.region_coverage {
        println!(
            "  region coverage: flat={:.0}%  edge={:.0}%  texture={:.0}%",
            cov.flat_fraction * 100.0,
            cov.strong_edge_fraction * 100.0,
            cov.textured_fraction * 100.0
        );
    }

    Ok(())
}
