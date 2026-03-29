/// Core run logic: load → process → save → diagnostics.
use anyhow::{bail, Context, Result};

use r3sizer_core::{AutoSharpParams, ClampPolicy, FitStrategy, MetricWeights, ProbeConfig, SharpenStrategy};
use r3sizer_io::{load_as_linear, save_from_linear};

use crate::{args::Cli, output::print_summary};

/// Resolve target dimensions from CLI args and input image size.
pub fn resolve_dimensions(args: &Cli, src_w: u32, src_h: u32) -> Result<(u32, u32)> {
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

/// Build pipeline params from CLI args + resolved dimensions.
pub fn build_params(args: &Cli, target_width: u32, target_height: u32) -> AutoSharpParams {
    let probe_strengths = if let Some(ref strengths) = args.probe_strengths {
        ProbeConfig::Explicit(strengths.clone())
    } else {
        ProbeConfig::Explicit(vec![0.05, 0.1, 0.2, 0.4, 0.8, 1.5, 3.0])
    };

    let metric_weights = if let Some(ref w) = args.metric_weights {
        if w.len() == 4 {
            MetricWeights {
                gamut_excursion: w[0],
                halo_ringing: w[1],
                edge_overshoot: w[2],
                texture_flattening: w[3],
            }
        } else {
            eprintln!("Warning: --metric-weights requires exactly 4 values, using defaults");
            MetricWeights::default()
        }
    } else {
        MetricWeights::default()
    };

    AutoSharpParams {
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
        metric_weights,
        diagnostics_level: args.diagnostics_level.into(),
        sharpen_strategy: SharpenStrategy::default(),
        ..Default::default()
    }
}

pub fn run(args: &Cli) -> Result<()> {
    let input_path = args.input.as_ref().expect("input is required in single-file mode");
    let output_path = args.output.as_ref().expect("output is required in single-file mode");

    // --- Load ---
    let input = load_as_linear(input_path)
        .with_context(|| format!("failed to load input file: {}", input_path.display()))?;

    // --- Resolve target dimensions ---
    let (target_width, target_height) = resolve_dimensions(args, input.width(), input.height())?;

    // --- Build params ---
    let params = build_params(args, target_width, target_height);

    // --- Process ---
    let output = r3sizer_core::process_auto_sharp_downscale(&input, &params)
        .context("pipeline processing failed")?;

    // --- Save image ---
    save_from_linear(&output.image, output_path)
        .with_context(|| format!("failed to save output file: {}", output_path.display()))?;

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
