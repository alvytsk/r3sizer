/// Formatted stdout output.
use r3sizer_core::{
    ArtifactMetric, AutoSharpDiagnostics, CrossingStatus, FallbackReason, FitStatus,
    MetricComponent, MetricMode, SelectionMode, SharpenMode,
};

/// Print a human-readable summary of the pipeline diagnostics to stdout.
pub fn print_summary(diag: &AutoSharpDiagnostics) {
    println!(
        "Output size                 : {}x{}",
        diag.output_size.width, diag.output_size.height
    );
    println!(
        "Sharpen mode                : {}",
        sharpen_mode_label(diag.sharpen_mode)
    );
    println!(
        "Metric mode                 : {}",
        metric_mode_label(diag.metric_mode)
    );
    println!(
        "Artifact metric             : {}",
        artifact_metric_label(diag.artifact_metric)
    );
    println!(
        "Baseline artifact ratio     : {:.6}",
        diag.baseline_artifact_ratio
    );
    println!(
        "Selected strength           : {:.4}",
        diag.selected_strength
    );
    println!(
        "Target metric value         : {:.6}",
        diag.target_artifact_ratio
    );
    println!(
        "Measured metric value        : {:.6}",
        diag.measured_metric_value
    );
    println!(
        "Measured artifact ratio     : {:.6}",
        diag.measured_artifact_ratio
    );
    println!(
        "Budget reachable            : {}",
        if diag.budget_reachable { "yes" } else { "no" }
    );
    println!(
        "Fit status                  : {}",
        fit_status_label(&diag.fit_status)
    );
    println!(
        "Crossing status             : {}",
        crossing_status_label(&diag.crossing_status)
    );
    println!(
        "Selection mode              : {}",
        selection_mode_label(&diag.selection_mode)
    );
    if let Some(reason) = &diag.fallback_reason {
        println!(
            "Fallback reason             : {}",
            fallback_reason_label(reason)
        );
    }

    // Selection and metric breakdown
    if let Some(ref mc) = diag.metric_components {
        println!();
        println!("Selection:");
        println!(
            "  metric                    : {}",
            metric_component_label(mc.selected_metric)
        );
        println!(
            "  selection_score           : {:.6}",
            mc.selection_score
        );
        println!();
        println!("Metric breakdown (v0.2 — diagnostic only):");
        for (component, value) in &mc.components {
            println!("  {:24}: {:.6}", metric_component_label(*component), value);
        }
        println!(
            "  composite_score           : {:.6}  (not used for selection)",
            mc.composite_score
        );
        println!(
            "  weights                   : [{:.1}, {:.1}, {:.1}, {:.1}]",
            diag.metric_weights.gamut_excursion,
            diag.metric_weights.halo_ringing,
            diag.metric_weights.edge_overshoot,
            diag.metric_weights.texture_flattening,
        );
    }

    // Fit quality
    if let Some(q) = &diag.fit_quality {
        println!();
        println!("Fit quality:");
        println!("  R²                        : {:.6}", q.r_squared);
        println!("  Residual sum of squares   : {:.2e}", q.residual_sum_of_squares);
        println!("  Max residual              : {:.2e}", q.max_residual);
        println!("  Min pivot                 : {:.2e}", q.min_pivot);
    }

    // Robustness
    if let Some(r) = &diag.robustness {
        println!();
        println!("Robustness:");
        println!("  Monotonic                 : {}", if r.monotonic { "yes" } else { "no" });
        println!("  Quasi-monotonic           : {}", if r.quasi_monotonic { "yes" } else { "no" });
        println!("  R² ok                     : {}", if r.r_squared_ok { "yes" } else { "no" });
        println!("  Well conditioned          : {}", if r.well_conditioned { "yes" } else { "no" });
        println!("  LOO stable                : {}", if r.loo_stable { "yes" } else { "no" });
        println!("  Max LOO root change       : {:.4}", r.max_loo_root_change);
    }

    // Timing
    let t = &diag.timing;
    if t.total_us > 0 {
        println!();
        println!("Timing (us):");
        println!("  Resize                    : {}", t.resize_us);
        println!("  Contrast                  : {}", t.contrast_us);
        println!("  Baseline                  : {}", t.baseline_us);
        println!("  Probing                   : {}", t.probing_us);
        println!("  Fit                       : {}", t.fit_us);
        println!("  Robustness                : {}", t.robustness_us);
        println!("  Final sharpen             : {}", t.final_sharpen_us);
        println!("  Clamp                     : {}", t.clamp_us);
        println!("  Total                     : {}", t.total_us);
    }

}

fn sharpen_mode_label(m: SharpenMode) -> &'static str {
    match m {
        SharpenMode::Rgb => "rgb",
        SharpenMode::Lightness => "lightness (CIE Y)",
    }
}

fn metric_mode_label(m: MetricMode) -> &'static str {
    match m {
        MetricMode::AbsoluteTotal => "absolute (total artifact ratio)",
        MetricMode::RelativeToBase => "relative (sharpening-added artifacts)",
    }
}

fn fit_status_label(s: &FitStatus) -> String {
    match s {
        FitStatus::Success => "success".to_string(),
        FitStatus::Failed { reason } => format!("failed ({reason})"),
        FitStatus::Skipped => "skipped".to_string(),
    }
}

fn crossing_status_label(s: &CrossingStatus) -> &'static str {
    match s {
        CrossingStatus::Found => "found",
        CrossingStatus::NotFoundInRange => "not found in range",
        CrossingStatus::NotAttempted => "not attempted",
    }
}

fn selection_mode_label(s: &SelectionMode) -> &'static str {
    match s {
        SelectionMode::PolynomialRoot => "polynomial root",
        SelectionMode::BestSampleWithinBudget => "best sample within budget",
        SelectionMode::LeastBadSample => "least bad sample",
        SelectionMode::BudgetUnreachable => "budget unreachable",
    }
}

fn artifact_metric_label(m: ArtifactMetric) -> &'static str {
    match m {
        ArtifactMetric::ChannelClippingRatio => "channel clipping ratio",
        ArtifactMetric::PixelOutOfGamutRatio => "pixel out-of-gamut ratio",
    }
}

fn fallback_reason_label(r: &FallbackReason) -> &'static str {
    match r {
        FallbackReason::FitFailed => "fit failed",
        FallbackReason::FitUnstable => "fit unstable (LOO)",
        FallbackReason::RootOutOfRange => "root out of range",
        FallbackReason::MetricNonMonotonic => "metric non-monotonic",
        FallbackReason::BudgetTooStrictForContent => "budget too strict for content",
        FallbackReason::DirectSearchConfigured => "direct search configured",
        FallbackReason::FitPoorQuality => "fit poor quality (R² < 0.85)",
    }
}

fn metric_component_label(c: MetricComponent) -> &'static str {
    match c {
        MetricComponent::GamutExcursion => "gamut_excursion",
        MetricComponent::HaloRinging => "halo_ringing",
        MetricComponent::EdgeOvershoot => "edge_overshoot",
        MetricComponent::TextureFlattening => "texture_flattening",
    }
}

