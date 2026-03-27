/// Formatted stdout output.
use imgsharp_core::{
    ArtifactMetric, AutoSharpDiagnostics, CrossingStatus, FitStatus, MetricMode, Provenance,
    SelectionMode, SharpenMode, SharpenModel,
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
        "Sharpen model               : {}",
        sharpen_model_label(diag.sharpen_model)
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
    println!();
    println!("Stage provenance:");
    println!("  Color conversion          : {}", provenance_label(diag.provenance.color_conversion));
    println!("  Resize                    : {}", provenance_label(diag.provenance.resize));
    println!("  Contrast leveling         : {}", provenance_label(diag.provenance.contrast_leveling));
    println!("  Sharpen operator          : {}", provenance_label(diag.provenance.sharpen_operator));
    println!("  Lightness reconstruction  : {}", provenance_label(diag.provenance.lightness_reconstruction));
    println!("  Artifact metric           : {}", provenance_label(diag.provenance.artifact_metric));
    println!("  Polynomial fit            : {}", provenance_label(diag.provenance.polynomial_fit));
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

fn sharpen_model_label(m: SharpenModel) -> &'static str {
    match m {
        SharpenModel::PracticalUsm => "practical USM",
        SharpenModel::PaperLightnessApprox => "paper lightness approx",
    }
}

fn artifact_metric_label(m: ArtifactMetric) -> &'static str {
    match m {
        ArtifactMetric::ChannelClippingRatio => "channel clipping ratio",
        ArtifactMetric::PixelOutOfGamutRatio => "pixel out-of-gamut ratio",
    }
}

fn provenance_label(p: Provenance) -> &'static str {
    match p {
        Provenance::PaperConfirmed => "paper confirmed",
        Provenance::PaperSupported => "paper supported",
        Provenance::EngineeringChoice => "engineering choice",
        Provenance::EngineeringProxy => "engineering proxy",
        Provenance::Placeholder => "placeholder",
    }
}
