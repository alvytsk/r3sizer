/// Named pipeline configuration presets.
///
/// Stable presets:
///   - **photo** (default): P0=0.003 — natural photographic content
///   - **precision**: P0=0.001 — text, UI, architecture, hard-edge preservation
///
/// Legacy presets (for A/B comparison):
///   - **baseline**: Minimal pipeline — uniform sharpen, no chroma guard, no evaluator
///   - **v3-adaptive**: Content-adaptive sharpening with default gain table
///   - **v5-full**: Full v5 pipeline — adaptive sharpen + chroma guard + saturation guard + evaluator
///   - **v5-two-pass**: Same as v5-full but with two-pass adaptive probing
use anyhow::Result;
use r3sizer_core::{
    AutoSharpParams, ChromaRegionFactors, ClassificationParams, EvaluatorConfig,
    ExperimentalSharpenMode, GainTable, ProbeConfig, SaturationGuardParams, SharpenStrategy,
};

/// List of available preset names (for help text / validation).
pub const PRESET_NAMES: &[&str] = &[
    "photo",
    "precision",
    "baseline",
    "v3-adaptive",
    "v5-full",
    "v5-two-pass",
];

/// Build pipeline params from a named preset.
pub fn preset_params(name: &str, tw: u32, th: u32) -> Result<AutoSharpParams, String> {
    match name {
        // ── Stable presets ──────────────────────────────────────────────
        "photo" => Ok(AutoSharpParams::photo(tw, th)),

        "precision" => Ok(AutoSharpParams::precision(tw, th)),

        // ── Legacy presets ──────────────────────────────────────────────
        "baseline" => Ok(AutoSharpParams {
            target_width: tw,
            target_height: th,
            sharpen_strategy: SharpenStrategy::Uniform,
            experimental_sharpen_mode: None,
            evaluator_config: None,
            ..AutoSharpParams::default()
        }),

        "v3-adaptive" => Ok(AutoSharpParams {
            target_width: tw,
            target_height: th,
            sharpen_strategy: SharpenStrategy::ContentAdaptive {
                classification: ClassificationParams::default(),
                gain_table: GainTable::v03_default(),
                max_backoff_iterations: 4,
                backoff_scale_factor: 0.8,
            },
            experimental_sharpen_mode: None,
            evaluator_config: None,
            ..AutoSharpParams::default()
        }),

        "v5-full" => Ok(AutoSharpParams {
            target_width: tw,
            target_height: th,
            sharpen_strategy: SharpenStrategy::ContentAdaptive {
                classification: ClassificationParams::default(),
                gain_table: GainTable::v03_default(),
                max_backoff_iterations: 4,
                backoff_scale_factor: 0.8,
            },
            experimental_sharpen_mode: Some(ExperimentalSharpenMode::LumaPlusChromaGuard {
                max_chroma_shift: 0.25,
                chroma_region_factors: Some(ChromaRegionFactors::default()),
                saturation_guard: Some(SaturationGuardParams::default()),
            }),
            evaluator_config: Some(EvaluatorConfig::Heuristic),
            ..AutoSharpParams::default()
        }),

        "v5-two-pass" => Ok(AutoSharpParams {
            target_width: tw,
            target_height: th,
            probe_strengths: ProbeConfig::TwoPass {
                coarse_count: 5,
                coarse_min: 0.003,
                coarse_max: 0.50,
                dense_count: 4,
                window_margin: 0.5,
            },
            sharpen_strategy: SharpenStrategy::ContentAdaptive {
                classification: ClassificationParams::default(),
                gain_table: GainTable::v03_default(),
                max_backoff_iterations: 4,
                backoff_scale_factor: 0.8,
            },
            experimental_sharpen_mode: Some(ExperimentalSharpenMode::LumaPlusChromaGuard {
                max_chroma_shift: 0.25,
                chroma_region_factors: Some(ChromaRegionFactors::default()),
                saturation_guard: Some(SaturationGuardParams::default()),
            }),
            evaluator_config: Some(EvaluatorConfig::Heuristic),
            ..AutoSharpParams::default()
        }),

        _ => Err(format!(
            "unknown preset '{name}'. Available: {}",
            PRESET_NAMES.join(", ")
        )),
    }
}

/// Print all preset names to stdout.
pub fn list_presets() -> Result<()> {
    println!("Available presets:");
    println!();
    println!("  Stable:");
    println!("    photo       P0=0.003 — natural photographic content (default)");
    println!("    precision   P0=0.001 — text, UI, architecture, hard edges");
    println!();
    println!("  Legacy (for A/B comparison):");
    println!("    baseline    Minimal: uniform sharpen, no chroma guard, no evaluator");
    println!("    v3-adaptive Content-adaptive sharpening with default gain table");
    println!("    v5-full     Full v5 pipeline: adaptive + chroma guard + saturation guard");
    println!("    v5-two-pass Same as v5-full with two-pass adaptive probing");
    Ok(())
}

/// Show the configuration for a named preset.
pub fn show_preset(name: &str) -> Result<()> {
    // Use a small dummy size to get representable params; sizes are not part of the preset.
    let params = preset_params(name, 800, 600).map_err(|e| anyhow::anyhow!("{e}"))?;
    let json = serde_json::to_string_pretty(&params)
        .map_err(|e| anyhow::anyhow!("failed to serialise preset: {e}"))?;
    println!("Preset '{name}':");
    println!("{json}");
    Ok(())
}
