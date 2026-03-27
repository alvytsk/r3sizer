/// Core run logic: load → process → save → diagnostics.
use anyhow::{bail, Context, Result};

use imgsharp_core::{AutoSharpParams, ClampPolicy, FitStrategy, ProbeConfig};
use imgsharp_io::{load_as_linear, save_from_linear};

use crate::{args::Cli, output::print_summary};

/// Resolve target dimensions from CLI args and input image size.
fn resolve_dimensions(args: &Cli, src_w: u32, src_h: u32) -> Result<(u32, u32)> {
    match (args.width, args.height, args.preserve_aspect_ratio) {
        (Some(w), Some(h), _) => Ok((w, h)),
        (Some(w), None, true) => {
            let h = ((w as f64 / src_w as f64) * src_h as f64).round() as u32;
            Ok((w, h.max(1)))
        }
        (None, Some(h), true) => {
            let w = ((h as f64 / src_h as f64) * src_w as f64).round() as u32;
            Ok((w.max(1), h))
        }
        (None, None, _) => bail!("at least one of --width or --height is required"),
        (_, None, false) | (None, _, false) => {
            bail!("both --width and --height are required unless --preserve-aspect-ratio is set")
        }
    }
}

pub fn run(args: &Cli) -> Result<()> {
    // --- Load ---
    let input = load_as_linear(&args.input)
        .with_context(|| format!("failed to load input file: {}", args.input.display()))?;

    // --- Resolve target dimensions ---
    let (target_width, target_height) = resolve_dimensions(args, input.width(), input.height())?;

    // --- Build params ---
    let probe_strengths = if let Some(ref strengths) = args.probe_strengths {
        ProbeConfig::Explicit(strengths.clone())
    } else {
        // Default: non-uniform list, denser near zero.
        ProbeConfig::Explicit(vec![0.05, 0.1, 0.2, 0.4, 0.8, 1.5, 3.0])
    };

    let params = AutoSharpParams {
        target_width,
        target_height,
        probe_strengths,
        target_artifact_ratio: args.target_artifact_ratio,
        enable_contrast_leveling: args.enable_contrast_leveling,
        sharpen_sigma: args.sharpen_sigma,
        fit_strategy: FitStrategy::Cubic,
        output_clamp: ClampPolicy::Clamp,
        sharpen_mode: args.sharpen_mode.into(),
        sharpen_model: args.sharpen_model.into(),
        metric_mode: args.metric_mode.into(),
        artifact_metric: args.artifact_metric.into(),
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
