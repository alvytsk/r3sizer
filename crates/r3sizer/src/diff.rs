/// Sweep comparison tool.
///
/// Reads two sweep summary JSON files and produces a structured diff report
/// showing per-file and aggregate changes.
///
/// Usage: `r3sizer --sweep-diff baseline.json,candidate.json`
use std::path::Path;

use anyhow::{Context, Result};

use crate::sweep::{AggregateStats, ComponentStats, FileResult, SweepSummary};

/// Run the diff between two sweep summaries.
pub fn run_diff(base_path: &Path, candidate_path: &Path) -> Result<()> {
    let base: SweepSummary = read_summary(base_path)?;
    let cand: SweepSummary = read_summary(candidate_path)?;

    let base_name = base_path.file_stem().unwrap_or_default().to_string_lossy();
    let cand_name = candidate_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();

    println!("Sweep comparison: {} → {}", base_name, cand_name);
    println!("{}", "=".repeat(72));

    // --- Aggregate summary ---
    print_aggregate_diff(&base.aggregate, &cand.aggregate, &base_name, &cand_name);

    // --- Per-file diff ---
    print_per_file_diff(&base.results, &cand.results, &base_name, &cand_name);

    // --- Regressions / improvements ---
    print_verdict(&base.results, &cand.results, &base_name, &cand_name);

    Ok(())
}

fn read_summary(path: &Path) -> Result<SweepSummary> {
    let data = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&data).with_context(|| format!("failed to parse {}", path.display()))
}

// ---------------------------------------------------------------------------
// Aggregate diff
// ---------------------------------------------------------------------------

fn print_aggregate_diff(
    base: &AggregateStats,
    cand: &AggregateStats,
    base_name: &str,
    cand_name: &str,
) {
    println!();
    println!("Aggregate");
    println!("{:-<72}", "");
    println!(
        "  {:30} {:>12} {:>12} {:>10}",
        "", base_name, cand_name, "delta"
    );
    println!("  {:30} {:>12} {:>12} {:>10}", "", "----", "----", "-----");

    agg_row(
        "files (ok/total)",
        &format!("{}/{}", base.succeeded, base.total_files),
        &format!("{}/{}", cand.succeeded, cand.total_files),
    );
    agg_row_f(
        "mean strength",
        base.mean_selected_strength,
        cand.mean_selected_strength,
    );
    agg_row_f(
        "median strength",
        base.median_selected_strength,
        cand.median_selected_strength,
    );
    agg_row_pct(
        "fit success rate",
        base.fit_success_rate,
        cand.fit_success_rate,
    );
    agg_row_f(
        "mean time (ms)",
        base.mean_total_us as f32 / 1000.0,
        cand.mean_total_us as f32 / 1000.0,
    );

    println!();
    println!("  Metric statistics (mean)");
    println!(
        "  {:30} {:>12} {:>12} {:>10}",
        "", base_name, cand_name, "delta%"
    );
    println!("  {:30} {:>12} {:>12} {:>10}", "", "----", "----", "------");

    let metrics: Vec<(&str, &ComponentStats, &ComponentStats)> = vec![
        (
            "gamut_excursion",
            &base.gamut_excursion,
            &cand.gamut_excursion,
        ),
        ("halo_ringing", &base.halo_ringing, &cand.halo_ringing),
        ("edge_overshoot", &base.edge_overshoot, &cand.edge_overshoot),
        (
            "texture_flattening",
            &base.texture_flattening,
            &cand.texture_flattening,
        ),
        (
            "composite_score",
            &base.composite_score,
            &cand.composite_score,
        ),
        ("ringing_score", &base.ringing_score, &cand.ringing_score),
        ("envelope_scale", &base.envelope_scale, &cand.envelope_scale),
        ("edge_retention", &base.edge_retention, &cand.edge_retention),
        (
            "chroma_clamped",
            &base.chroma_clamped_fraction,
            &cand.chroma_clamped_fraction,
        ),
    ];

    for (name, bs, cs) in &metrics {
        let pct = if bs.mean.abs() > 1e-9 {
            format!("{:+.1}%", ((cs.mean - bs.mean) / bs.mean) * 100.0)
        } else if cs.mean.abs() > 1e-9 {
            "new".to_string()
        } else {
            "—".to_string()
        };
        println!(
            "  {:<30} {:>12.5} {:>12.5} {:>10}",
            name, bs.mean, cs.mean, pct
        );
    }
}

fn agg_row(label: &str, base_val: &str, cand_val: &str) {
    println!("  {:<30} {:>12} {:>12}", label, base_val, cand_val);
}

fn agg_row_f(label: &str, base_val: f32, cand_val: f32) {
    let delta = cand_val - base_val;
    println!(
        "  {:<30} {:>12.4} {:>12.4} {:>+10.4}",
        label, base_val, cand_val, delta
    );
}

fn agg_row_pct(label: &str, base_val: f32, cand_val: f32) {
    println!(
        "  {:<30} {:>11.1}% {:>11.1}% {:>+9.1}pp",
        label,
        base_val * 100.0,
        cand_val * 100.0,
        (cand_val - base_val) * 100.0,
    );
}

// ---------------------------------------------------------------------------
// Per-file diff
// ---------------------------------------------------------------------------

fn print_per_file_diff(
    base_results: &[FileResult],
    cand_results: &[FileResult],
    base_name: &str,
    cand_name: &str,
) {
    println!();
    println!("Per-file strength & mode");
    println!("{:-<72}", "");
    println!(
        "  {:<25} {:>8} {:>8} {:>8} {:>8} {:<8}",
        "image", base_name, cand_name, "delta", "mode_b", "mode_c"
    );
    println!(
        "  {:<25} {:>8} {:>8} {:>8} {:>8} {:<8}",
        "-----", "----", "----", "-----", "------", "------"
    );

    let base_map: std::collections::HashMap<String, &FileResult> = base_results
        .iter()
        .map(|r| (file_key(&r.input), r))
        .collect();

    for cr in cand_results {
        let key = file_key(&cr.input);
        let name = short_name(&cr.input);
        if let Some(br) = base_map.get(&key) {
            let delta = cr.selected_strength - br.selected_strength;
            let flag = if delta.abs() > 0.05 { " *" } else { "" };
            let mode_b = mode_short(&br.selection_mode);
            let mode_c = mode_short(&cr.selection_mode);
            let mode_change = if mode_b != mode_c {
                format!("{} → {}", mode_b, mode_c)
            } else {
                mode_b.to_string()
            };
            println!(
                "  {:<25} {:>8.4} {:>8.4} {:>+8.4} {}{}",
                name, br.selected_strength, cr.selected_strength, delta, mode_change, flag
            );
        } else {
            println!(
                "  {:<25} {:>8} {:>8.4} {:>8} {}",
                name,
                "—",
                cr.selected_strength,
                "new",
                mode_short(&cr.selection_mode)
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Verdict: improvements & regressions
// ---------------------------------------------------------------------------

fn print_verdict(
    base_results: &[FileResult],
    cand_results: &[FileResult],
    _base_name: &str,
    _cand_name: &str,
) {
    let base_map: std::collections::HashMap<String, &FileResult> = base_results
        .iter()
        .map(|r| (file_key(&r.input), r))
        .collect();

    let mut improvements = Vec::new();
    let mut regressions = Vec::new();
    let mut unchanged = 0;

    for cr in cand_results {
        let key = file_key(&cr.input);
        if let Some(br) = base_map.get(&key) {
            let name = short_name(&cr.input);
            let cs_delta = cr.composite_score - br.composite_score;
            let s_delta = cr.selected_strength - br.selected_strength;

            if cs_delta < -0.001 || (cs_delta.abs() < 0.001 && s_delta.abs() > 0.05) {
                improvements.push((name, cs_delta, s_delta));
            } else if cs_delta > 0.001 {
                regressions.push((name, cs_delta, s_delta));
            } else {
                unchanged += 1;
            }
        }
    }

    println!();
    println!("Verdict");
    println!("{:-<72}", "");

    if improvements.is_empty() && regressions.is_empty() {
        println!(
            "  No significant changes across {} images.",
            unchanged + improvements.len() + regressions.len()
        );
        return;
    }

    if !improvements.is_empty() {
        println!("  Improvements ({}):", improvements.len());
        for (name, cs, s) in &improvements {
            println!("    {:<25} composite {:+.5}  strength {:+.4}", name, cs, s);
        }
    }

    if !regressions.is_empty() {
        println!("  Regressions ({}):", regressions.len());
        for (name, cs, s) in &regressions {
            println!("    {:<25} composite {:+.5}  strength {:+.4}", name, cs, s);
        }
    }

    println!("  Unchanged: {}", unchanged);

    let total = improvements.len() + regressions.len() + unchanged;
    if regressions.is_empty() {
        println!("\n  ✓ No regressions across {total} images.");
    } else {
        println!(
            "\n  ⚠ {}/{total} images regressed. Review before merging.",
            regressions.len()
        );
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn file_key(path: &str) -> String {
    Path::new(path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase()
}

fn short_name(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string()
}

fn mode_short(mode: &r3sizer_core::SelectionMode) -> &'static str {
    match mode {
        r3sizer_core::SelectionMode::PolynomialRoot => "poly",
        r3sizer_core::SelectionMode::BestSampleWithinBudget => "best",
        r3sizer_core::SelectionMode::LeastBadSample => "least",
        r3sizer_core::SelectionMode::BudgetUnreachable => "unreach",
    }
}
