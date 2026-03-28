/// Sweep mode: batch process a directory of images and produce a summary.
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;

use r3sizer_core::SelectionMode;
use r3sizer_io::{load_as_linear, save_from_linear};

use crate::args::Cli;
use crate::run::{build_params, resolve_dimensions};

/// Supported image extensions for sweep mode.
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "tiff", "tif", "bmp", "webp"];

/// Per-file result in the sweep summary.
#[derive(Debug, Serialize)]
struct FileResult {
    input: String,
    output: Option<String>,
    selected_strength: f32,
    selection_mode: r3sizer_core::SelectionMode,
    fallback_reason: Option<r3sizer_core::FallbackReason>,
    measured_artifact_ratio: f32,
    measured_metric_value: f32,
    fit_r_squared: Option<f64>,
    monotonic: Option<bool>,
    total_us: u64,
}

/// Error entry for a file that failed processing.
#[derive(Debug, Serialize)]
struct FileError {
    input: String,
    error: String,
}

/// Aggregate statistics across all successfully processed files.
#[derive(Debug, Serialize)]
struct AggregateStats {
    total_files: usize,
    succeeded: usize,
    failed: usize,
    mean_selected_strength: f32,
    median_selected_strength: f32,
    mean_total_us: f64,
    selection_mode_counts: SelectionModeCounts,
    fit_success_rate: f32,
}

#[derive(Debug, Default, Serialize)]
struct SelectionModeCounts {
    polynomial_root: usize,
    best_sample_within_budget: usize,
    least_bad_sample: usize,
    budget_unreachable: usize,
}

/// Top-level sweep summary written to JSON.
#[derive(Debug, Serialize)]
struct SweepSummary {
    aggregate: AggregateStats,
    results: Vec<FileResult>,
    errors: Vec<FileError>,
}

/// Run sweep mode: process all images in a directory.
pub fn run_sweep(args: &Cli) -> Result<()> {
    let sweep_dir = args.sweep_dir.as_ref().expect("sweep_dir is required");
    if !sweep_dir.is_dir() {
        bail!("--sweep-dir path is not a directory: {}", sweep_dir.display());
    }

    let files = find_images(sweep_dir)?;
    if files.is_empty() {
        bail!("no image files found in {}", sweep_dir.display());
    }

    // Create output directory if specified.
    if let Some(ref out_dir) = args.sweep_output_dir {
        std::fs::create_dir_all(out_dir)
            .with_context(|| format!("failed to create output directory: {}", out_dir.display()))?;
    }

    println!("Sweep: processing {} files from {}", files.len(), sweep_dir.display());

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
    println!("Sweep complete: {} succeeded, {} failed", aggregate.succeeded, aggregate.failed);
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
    if let Some(ref summary_path) = args.sweep_summary {
        let summary = SweepSummary { aggregate, results, errors };
        let json = serde_json::to_string_pretty(&summary)
            .context("failed to serialise sweep summary")?;
        std::fs::write(summary_path, json)
            .with_context(|| format!("failed to write sweep summary to {}", summary_path.display()))?;
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
fn process_one(args: &Cli, input_path: &Path) -> Result<FileResult> {
    let input = load_as_linear(input_path)
        .with_context(|| format!("failed to load: {}", input_path.display()))?;

    let (tw, th) = resolve_dimensions(args, input.width(), input.height())?;
    let params = build_params(args, tw, th);

    let output = r3sizer_core::process_auto_sharp_downscale(&input, &params)
        .context("pipeline failed")?;

    // Save output image if sweep_output_dir is set.
    let output_path = if let Some(ref out_dir) = args.sweep_output_dir {
        let stem = input_path.file_stem().unwrap_or_default();
        let out_file = out_dir.join(format!("{}.png", stem.to_string_lossy()));
        save_from_linear(&output.image, &out_file)
            .with_context(|| format!("failed to save: {}", out_file.display()))?;
        Some(out_file.display().to_string())
    } else {
        None
    };

    let diag = &output.diagnostics;
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
    })
}

fn compute_aggregate(results: &[FileResult], failed: usize) -> AggregateStats {
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
    }
}
