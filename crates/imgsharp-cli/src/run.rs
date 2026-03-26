/// Core run logic: load → process → save → diagnostics.
use anyhow::{Context, Result};

use imgsharp_core::{AutoSharpParams, ClampPolicy, FitStrategy, ProbeConfig};
use imgsharp_io::{load_as_linear, save_from_linear};

use crate::{args::Cli, output::print_summary};

pub fn run(args: &Cli) -> Result<()> {
    // --- Load ---
    let input = load_as_linear(&args.input)
        .with_context(|| format!("failed to load input file: {}", args.input.display()))?;

    // --- Build params ---
    let probe_strengths = if let Some(ref strengths) = args.probe_strengths {
        ProbeConfig::Explicit(strengths.clone())
    } else {
        // Default: non-uniform list, denser near zero.
        ProbeConfig::Explicit(vec![0.05, 0.1, 0.2, 0.4, 0.8, 1.5, 3.0])
    };

    let params = AutoSharpParams {
        target_width: args.width,
        target_height: args.height,
        probe_strengths,
        target_artifact_ratio: args.target_artifact_ratio,
        enable_contrast_leveling: args.enable_contrast_leveling,
        sharpen_sigma: args.sharpen_sigma,
        fit_strategy: FitStrategy::Cubic,
        output_clamp: ClampPolicy::Clamp,
        sharpen_mode: args.sharpen_mode.into(),
        metric_mode: args.metric_mode.into(),
    };

    // --- Process ---
    let output = imgsharp_core::process_auto_sharp_downscale(&input, &params)
        .context("pipeline processing failed")?;

    // --- Save image ---
    save_from_linear(&output.image, &args.output)
        .with_context(|| format!("failed to save output file: {}", args.output.display()))?;

    // --- Print summary ---
    print_summary(&output.diagnostics);

    // --- Optionally write diagnostics JSON ---
    if let Some(ref diag_path) = args.diagnostics {
        let json = serde_json::to_string_pretty(&output.diagnostics)
            .context("failed to serialise diagnostics")?;
        std::fs::write(diag_path, json)
            .with_context(|| format!("failed to write diagnostics to {}", diag_path.display()))?;
        println!("Diagnostics written to     : {}", diag_path.display());
    }

    Ok(())
}
