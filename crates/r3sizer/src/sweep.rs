/// Sweep mode: batch process a directory of images and produce a summary.
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use r3sizer_core::SelectionMode;
use r3sizer_io::{load_as_linear, save_from_linear};

use crate::args::SweepArgs;
use crate::run::{build_params, resolve_dimensions};

/// Supported image extensions for sweep mode.
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "tiff", "tif", "bmp", "webp"];

/// Per-file result in the sweep summary.
#[derive(Debug, Serialize, Deserialize)]
pub struct FileResult {
    pub input: String,
    pub output: Option<String>,
    pub selected_strength: f32,
    pub selection_mode: r3sizer_core::SelectionMode,
    pub fallback_reason: Option<r3sizer_core::FallbackReason>,
    pub measured_artifact_ratio: f32,
    pub measured_metric_value: f32,
    pub fit_r_squared: Option<f64>,
    pub monotonic: Option<bool>,
    pub total_us: u64,
    pub gamut_excursion: f32,
    pub halo_ringing: f32,
    pub edge_overshoot: f32,
    pub texture_flattening: f32,
    pub composite_score: f32,
    pub ringing_score: f32,
    pub envelope_scale: f32,
    pub edge_retention: f32,
    pub texture_retention: f32,
    pub effective_target_artifact_ratio: f32,
    pub chroma_clamped_fraction: Option<f32>,
    pub chroma_effective_threshold_mean: Option<f32>,
}

/// Error entry for a file that failed processing.
#[derive(Debug, Serialize, Deserialize)]
pub struct FileError {
    input: String,
    error: String,
}

/// Per-component aggregate statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentStats {
    pub mean: f32,
    pub median: f32,
    pub p90: f32,
    pub p95: f32,
}

/// Aggregate statistics across all successfully processed files.
#[derive(Debug, Serialize, Deserialize)]
pub struct AggregateStats {
    pub total_files: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub mean_selected_strength: f32,
    pub median_selected_strength: f32,
    pub mean_total_us: f64,
    pub selection_mode_counts: SelectionModeCounts,
    pub fit_success_rate: f32,
    pub gamut_excursion: ComponentStats,
    pub halo_ringing: ComponentStats,
    pub edge_overshoot: ComponentStats,
    pub texture_flattening: ComponentStats,
    pub composite_score: ComponentStats,
    pub ringing_score: ComponentStats,
    pub envelope_scale: ComponentStats,
    pub edge_retention: ComponentStats,
    pub texture_retention: ComponentStats,
    pub effective_target_artifact_ratio: ComponentStats,
    pub chroma_clamped_fraction: ComponentStats,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SelectionModeCounts {
    polynomial_root: usize,
    best_sample_within_budget: usize,
    least_bad_sample: usize,
    budget_unreachable: usize,
}

/// Top-level sweep summary written to JSON.
#[derive(Debug, Serialize, Deserialize)]
pub struct SweepSummary {
    pub aggregate: AggregateStats,
    pub results: Vec<FileResult>,
    pub errors: Vec<FileError>,
}

/// Run sweep mode: process all images in a directory.
pub fn run_sweep(args: &SweepArgs) -> Result<()> {
    if !args.in_dir.is_dir() {
        bail!(
            "--in-dir path is not a directory: {}",
            args.in_dir.display()
        );
    }

    let files = find_images(&args.in_dir)?;
    if files.is_empty() {
        bail!("no image files found in {}", args.in_dir.display());
    }

    // Create output directory if specified.
    if let Some(ref out_dir) = args.out_dir {
        std::fs::create_dir_all(out_dir)
            .with_context(|| format!("failed to create output directory: {}", out_dir.display()))?;
    }

    println!(
        "Sweep: processing {} files from {}",
        files.len(),
        args.in_dir.display()
    );

    let mut results = Vec::new();
    let mut errors = Vec::new();

    for (i, file_path) in files.iter().enumerate() {
        print!("  [{}/{}] {} ... ", i + 1, files.len(), file_path.display());

        match process_one(args, file_path) {
            Ok(result) => {
                println!(
                    "ok (s={:.4}, mode={:?}, {:.1}ms)",
                    result.selected_strength,
                    result.selection_mode,
                    result.total_us as f64 / 1000.0,
                );
                results.push(result);
            }
            Err(e) => {
                println!("FAILED: {e:#}");
                errors.push(FileError {
                    input: file_path.display().to_string(),
                    error: format!("{e:#}"),
                });
            }
        }
    }

    // Compute aggregates.
    let aggregate = compute_aggregate(&results, errors.len());

    println!();
    println!(
        "Sweep complete: {} succeeded, {} failed",
        aggregate.succeeded, aggregate.failed
    );
    println!(
        "  Mean strength   : {:.4}",
        aggregate.mean_selected_strength
    );
    println!(
        "  Median strength : {:.4}",
        aggregate.median_selected_strength
    );
    println!(
        "  Fit success rate: {:.1}%",
        aggregate.fit_success_rate * 100.0
    );
    println!(
        "  Mean time       : {:.1}ms",
        aggregate.mean_total_us / 1000.0
    );

    // Write summary JSON.
    if let Some(ref summary_path) = args.summary {
        let summary = SweepSummary {
            aggregate,
            results,
            errors,
        };
        let json =
            serde_json::to_string_pretty(&summary).context("failed to serialise sweep summary")?;
        std::fs::write(summary_path, json).with_context(|| {
            format!(
                "failed to write sweep summary to {}",
                summary_path.display()
            )
        })?;
        println!("Summary written to         : {}", summary_path.display());
    }

    Ok(())
}

/// Find all image files in a directory (non-recursive).
fn find_images(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for ext in IMAGE_EXTENSIONS {
        let pattern = format!("{}/*.{}", dir.display(), ext);
        for path in glob::glob(&pattern)
            .with_context(|| format!("invalid glob pattern: {pattern}"))?
            .flatten()
        {
            files.push(path);
        }
        // Also match uppercase extensions.
        let pattern_upper = format!("{}/*.{}", dir.display(), ext.to_uppercase());
        for path in glob::glob(&pattern_upper)
            .with_context(|| format!("invalid glob pattern: {pattern_upper}"))?
            .flatten()
        {
            files.push(path);
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

/// Process a single file and return the result summary.
fn process_one(args: &SweepArgs, input_path: &Path) -> Result<FileResult> {
    let input = load_as_linear(input_path)
        .with_context(|| format!("failed to load: {}", input_path.display()))?;

    let (tw, th) = resolve_dimensions(&args.pipeline, input.width(), input.height())?;
    let params = build_params(&args.pipeline, tw, th);

    let output =
        r3sizer_core::process_auto_sharp_downscale(&input, &params).context("pipeline failed")?;

    // Save output image if out_dir is set.
    let output_path = if let Some(ref out_dir) = args.out_dir {
        let stem = input_path.file_stem().unwrap_or_default();
        let out_file = out_dir.join(format!("{}.png", stem.to_string_lossy()));
        save_from_linear(&output.image, &out_file)
            .with_context(|| format!("failed to save: {}", out_file.display()))?;
        Some(out_file.display().to_string())
    } else {
        None
    };

    let diag = &output.diagnostics;
    let (ge, hr, eo, tf, cs) = if let Some(ref mc) = diag.metric_components {
        (
            mc.components
                .get(&r3sizer_core::MetricComponent::GamutExcursion)
                .copied()
                .unwrap_or(0.0),
            mc.components
                .get(&r3sizer_core::MetricComponent::HaloRinging)
                .copied()
                .unwrap_or(0.0),
            mc.components
                .get(&r3sizer_core::MetricComponent::EdgeOvershoot)
                .copied()
                .unwrap_or(0.0),
            mc.components
                .get(&r3sizer_core::MetricComponent::TextureFlattening)
                .copied()
                .unwrap_or(0.0),
            mc.composite_score,
        )
    } else {
        (0.0, 0.0, 0.0, 0.0, 0.0)
    };
    // Step 4: base resize quality
    let (ringing_score, envelope_scale, edge_retention, texture_retention) =
        if let Some(bq) = diag.base_resize_quality {
            (
                bq.ringing_score,
                bq.envelope_scale,
                bq.edge_retention,
                bq.texture_retention,
            )
        } else {
            (0.0, 1.0, 1.0, 1.0)
        };

    // Step 5: chroma guard
    let chroma_clamped_fraction = diag
        .chroma_guard
        .as_ref()
        .map(|cg| cg.pixels_clamped_fraction);
    let chroma_effective_threshold_mean = diag
        .chroma_guard
        .as_ref()
        .and_then(|cg| cg.effective_threshold_mean);

    Ok(FileResult {
        input: input_path.display().to_string(),
        output: output_path,
        selected_strength: diag.selected_strength,
        selection_mode: diag.selection_mode.clone(),
        fallback_reason: diag.fallback_reason,
        measured_artifact_ratio: diag.measured_artifact_ratio,
        measured_metric_value: diag.measured_metric_value,
        fit_r_squared: diag.fit_quality.map(|q| q.r_squared),
        monotonic: diag.robustness.map(|r| r.monotonic),
        total_us: diag.timing.total_us,
        gamut_excursion: ge,
        halo_ringing: hr,
        edge_overshoot: eo,
        texture_flattening: tf,
        composite_score: cs,
        ringing_score,
        envelope_scale,
        edge_retention,
        texture_retention,
        effective_target_artifact_ratio: diag.effective_target_artifact_ratio,
        chroma_clamped_fraction,
        chroma_effective_threshold_mean,
    })
}

fn percentile(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = (p / 100.0 * (sorted.len() - 1) as f32).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn compute_component_stats(values: &[f32]) -> ComponentStats {
    if values.is_empty() {
        return ComponentStats {
            mean: 0.0,
            median: 0.0,
            p90: 0.0,
            p95: 0.0,
        };
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = sorted.len();
    let mean = sorted.iter().sum::<f32>() / n as f32;
    let median = if n.is_multiple_of(2) {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    } else {
        sorted[n / 2]
    };
    ComponentStats {
        mean,
        median,
        p90: percentile(&sorted, 90.0),
        p95: percentile(&sorted, 95.0),
    }
}

fn compute_aggregate(results: &[FileResult], failed: usize) -> AggregateStats {
    let empty_cs = ComponentStats {
        mean: 0.0,
        median: 0.0,
        p90: 0.0,
        p95: 0.0,
    };
    let n = results.len();
    if n == 0 {
        return AggregateStats {
            total_files: failed,
            succeeded: 0,
            failed,
            mean_selected_strength: 0.0,
            median_selected_strength: 0.0,
            mean_total_us: 0.0,
            selection_mode_counts: SelectionModeCounts::default(),
            fit_success_rate: 0.0,
            gamut_excursion: empty_cs.clone(),
            halo_ringing: empty_cs.clone(),
            edge_overshoot: empty_cs.clone(),
            texture_flattening: empty_cs.clone(),
            composite_score: empty_cs.clone(),
            ringing_score: empty_cs.clone(),
            envelope_scale: empty_cs.clone(),
            edge_retention: empty_cs.clone(),
            texture_retention: empty_cs.clone(),
            effective_target_artifact_ratio: empty_cs.clone(),
            chroma_clamped_fraction: empty_cs,
        };
    }

    let mut strengths: Vec<f32> = results.iter().map(|r| r.selected_strength).collect();
    strengths.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let mean_strength = strengths.iter().sum::<f32>() / n as f32;
    let median_strength = if n.is_multiple_of(2) {
        (strengths[n / 2 - 1] + strengths[n / 2]) / 2.0
    } else {
        strengths[n / 2]
    };

    let mean_us = results.iter().map(|r| r.total_us as f64).sum::<f64>() / n as f64;

    let mut counts = SelectionModeCounts::default();
    for r in results {
        match r.selection_mode {
            SelectionMode::PolynomialRoot => counts.polynomial_root += 1,
            SelectionMode::BestSampleWithinBudget => counts.best_sample_within_budget += 1,
            SelectionMode::LeastBadSample => counts.least_bad_sample += 1,
            SelectionMode::BudgetUnreachable => counts.budget_unreachable += 1,
        }
    }

    let fit_success_rate = counts.polynomial_root as f32 / n as f32;

    AggregateStats {
        total_files: n + failed,
        succeeded: n,
        failed,
        mean_selected_strength: mean_strength,
        median_selected_strength: median_strength,
        mean_total_us: mean_us,
        selection_mode_counts: counts,
        fit_success_rate,
        gamut_excursion: compute_component_stats(
            &results
                .iter()
                .map(|r| r.gamut_excursion)
                .collect::<Vec<_>>(),
        ),
        halo_ringing: compute_component_stats(
            &results.iter().map(|r| r.halo_ringing).collect::<Vec<_>>(),
        ),
        edge_overshoot: compute_component_stats(
            &results.iter().map(|r| r.edge_overshoot).collect::<Vec<_>>(),
        ),
        texture_flattening: compute_component_stats(
            &results
                .iter()
                .map(|r| r.texture_flattening)
                .collect::<Vec<_>>(),
        ),
        composite_score: compute_component_stats(
            &results
                .iter()
                .map(|r| r.composite_score)
                .collect::<Vec<_>>(),
        ),
        ringing_score: compute_component_stats(
            &results.iter().map(|r| r.ringing_score).collect::<Vec<_>>(),
        ),
        envelope_scale: compute_component_stats(
            &results.iter().map(|r| r.envelope_scale).collect::<Vec<_>>(),
        ),
        edge_retention: compute_component_stats(
            &results.iter().map(|r| r.edge_retention).collect::<Vec<_>>(),
        ),
        texture_retention: compute_component_stats(
            &results
                .iter()
                .map(|r| r.texture_retention)
                .collect::<Vec<_>>(),
        ),
        effective_target_artifact_ratio: compute_component_stats(
            &results
                .iter()
                .map(|r| r.effective_target_artifact_ratio)
                .collect::<Vec<_>>(),
        ),
        chroma_clamped_fraction: compute_component_stats(
            &results
                .iter()
                .filter_map(|r| r.chroma_clamped_fraction)
                .collect::<Vec<_>>(),
        ),
    }
}
