//! Deterministic recommendation engine (v0.5).
//!
//! Translates pipeline diagnostics + current params into actionable
//! [`Recommendation`]s.  Each recommendation carries a self-contained
//! [`ParamPatch`] that the UI can apply directly via `updateParams(patch)`.
//!
//! The polynomial solver remains authoritative — recommendations modify
//! *settings*, not the solver's output.

use crate::types::{
    AutoSharpDiagnostics, AutoSharpParams, ClassificationParams, GainTable, ParamPatch,
    ProbeConfig, Recommendation, RecommendationKind, Severity, SharpenMode, SharpenStrategy,
};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Generate recommendations from fully-assembled diagnostics and current params.
///
/// Called unconditionally after the pipeline finishes.  Rules that need
/// `evaluator_result` skip gracefully when it is `None`.
///
/// **Invariant:** no two returned recommendations touch the same `ParamPatch`
/// field.  This is enforced by construction (rules 1 & 2 are mutually
/// exclusive on `sharpen_strategy`, and all other rules target distinct fields).
pub fn generate_recommendations(
    diag: &AutoSharpDiagnostics,
    params: &AutoSharpParams,
) -> Vec<Recommendation> {
    let mut recs = Vec::new();

    rule_switch_to_content_adaptive(diag, params, &mut recs);
    rule_lower_strong_edge_gain(diag, params, &mut recs);
    rule_raise_artifact_budget(diag, params, &mut recs);
    rule_switch_to_lightness(diag, params, &mut recs);
    rule_widen_probe_range(diag, params, &mut recs);
    rule_lower_sigma(diag, params, &mut recs);

    recs
}

// ---------------------------------------------------------------------------
// Rule 1 — SwitchToContentAdaptive
// ---------------------------------------------------------------------------

fn rule_switch_to_content_adaptive(
    diag: &AutoSharpDiagnostics,
    params: &AutoSharpParams,
    recs: &mut Vec<Recommendation>,
) {
    if !matches!(params.sharpen_strategy, SharpenStrategy::Uniform) {
        return;
    }
    let features = match diag.evaluator_result.as_ref() {
        Some(e) => &e.features,
        None => return,
    };
    if features.edge_density <= 0.15 {
        return;
    }

    let confidence = ((features.edge_density - 0.15).min(0.25) / 0.25).clamp(0.0, 1.0);
    let mut gain_table = GainTable::v03_default();
    gain_table.strong_edge = 0.5;

    recs.push(Recommendation {
        kind: RecommendationKind::SwitchToContentAdaptive,
        severity: Severity::Suggestion,
        confidence,
        reason: format!(
            "Image has high edge density ({:.0}%). Content-adaptive sharpening \
             reduces halos by varying strength per region.",
            features.edge_density * 100.0,
        ),
        patch: ParamPatch {
            sharpen_strategy: Some(SharpenStrategy::ContentAdaptive {
                classification: ClassificationParams::default(),
                gain_table,
                max_backoff_iterations: 4,
                backoff_scale_factor: 0.8,
            }),
            ..ParamPatch::default()
        },
    });
}

// ---------------------------------------------------------------------------
// Rule 2 — LowerStrongEdgeGain
// ---------------------------------------------------------------------------

fn rule_lower_strong_edge_gain(
    diag: &AutoSharpDiagnostics,
    params: &AutoSharpParams,
    recs: &mut Vec<Recommendation>,
) {
    let (classification, gain_table, max_backoff, backoff_scale) = match &params.sharpen_strategy {
        SharpenStrategy::ContentAdaptive {
            classification,
            gain_table,
            max_backoff_iterations,
            backoff_scale_factor,
        } => (classification, gain_table, *max_backoff_iterations, *backoff_scale_factor),
        _ => return,
    };

    let coverage = match diag.region_coverage.as_ref() {
        Some(c) => c,
        None => return,
    };

    if coverage.risky_halo_zone_fraction <= 0.15 || gain_table.strong_edge <= 0.5 {
        return;
    }

    let fraction = coverage.risky_halo_zone_fraction;
    let severity = if fraction > 0.25 {
        Severity::Warning
    } else {
        Severity::Suggestion
    };
    let confidence = ((fraction - 0.15).min(0.25) / 0.25).clamp(0.0, 1.0);

    let mut new_gain = *gain_table;
    new_gain.strong_edge = (gain_table.strong_edge * 0.5).max(GainTable::MIN_GAIN_VALUE);

    recs.push(Recommendation {
        kind: RecommendationKind::LowerStrongEdgeGain,
        severity,
        confidence,
        reason: format!(
            "{:.0}% of the image is in the halo risk zone. Reducing strong-edge gain \
             will temper sharpening near prominent edges.",
            fraction * 100.0,
        ),
        patch: ParamPatch {
            sharpen_strategy: Some(SharpenStrategy::ContentAdaptive {
                classification: *classification,
                gain_table: new_gain,
                max_backoff_iterations: max_backoff,
                backoff_scale_factor: backoff_scale,
            }),
            ..ParamPatch::default()
        },
    });
}

// ---------------------------------------------------------------------------
// Rule 3 — RaiseArtifactBudget
// ---------------------------------------------------------------------------

fn rule_raise_artifact_budget(
    diag: &AutoSharpDiagnostics,
    params: &AutoSharpParams,
    recs: &mut Vec<Recommendation>,
) {
    let eval = match diag.evaluator_result.as_ref() {
        Some(e) => e,
        None => return,
    };
    let s_eval = match eval.suggested_strength {
        Some(s) => s,
        None => return,
    };

    let s_cur = diag.selected_strength;
    if s_cur <= 0.0 {
        return;
    }
    if (s_eval - s_cur).abs() / s_cur <= 0.30 {
        return;
    }

    // Evaluate the fitted curve at s_eval to get the corresponding budget.
    let new_p0 = if let Some(coeffs) = &diag.fit_coefficients {
        // Check s_eval is within probe range.
        let (s_min, s_max) = probe_range(&diag.probe_samples);
        if s_eval < s_min || s_eval > s_max {
            return; // Outside range — WidenProbeRange handles this.
        }
        coeffs.evaluate(s_eval as f64) as f32
    } else {
        // No fit — interpolate from nearest probe samples.
        match interpolate_from_probes(&diag.probe_samples, s_eval) {
            Some(v) => v,
            None => return,
        }
    };

    let new_p0 = new_p0.clamp(1e-5, 0.05);

    // Don't recommend if the change is trivial.
    let old_p0 = params.target_artifact_ratio;
    if (new_p0 - old_p0).abs() / old_p0.max(1e-8) < 0.1 {
        return;
    }

    recs.push(Recommendation {
        kind: RecommendationKind::RaiseArtifactBudget,
        severity: Severity::Info,
        confidence: eval.confidence,
        reason: format!(
            "Adjusting artifact budget to {:.1e} (from {:.1e}) lets the solver \
             target strength {:.3} naturally.",
            new_p0, old_p0, s_eval,
        ),
        patch: ParamPatch {
            target_artifact_ratio: Some(new_p0),
            ..ParamPatch::default()
        },
    });
}

// ---------------------------------------------------------------------------
// Rule 4 — SwitchToLightness
// ---------------------------------------------------------------------------

fn rule_switch_to_lightness(
    diag: &AutoSharpDiagnostics,
    params: &AutoSharpParams,
    recs: &mut Vec<Recommendation>,
) {
    if params.sharpen_mode != SharpenMode::Rgb {
        return;
    }
    let features = match diag.evaluator_result.as_ref() {
        Some(e) => &e.features,
        None => return,
    };
    if features.edge_density <= 0.10 {
        return;
    }

    let confidence = ((features.edge_density - 0.10).min(0.20) / 0.20).clamp(0.0, 1.0);
    recs.push(Recommendation {
        kind: RecommendationKind::SwitchToLightness,
        severity: Severity::Suggestion,
        confidence,
        reason: "RGB sharpening on edge-rich content risks color fringing. \
                 Lightness mode sharpens luminance only."
            .into(),
        patch: ParamPatch {
            sharpen_mode: Some(SharpenMode::Lightness),
            ..ParamPatch::default()
        },
    });
}

// ---------------------------------------------------------------------------
// Rule 5 — WidenProbeRange
// ---------------------------------------------------------------------------

fn rule_widen_probe_range(
    diag: &AutoSharpDiagnostics,
    _params: &AutoSharpParams,
    recs: &mut Vec<Recommendation>,
) {
    let (s_min, s_max) = probe_range(&diag.probe_samples);
    if s_min >= s_max {
        return;
    }

    // Trigger A: poor R²
    let poor_fit = diag
        .fit_quality
        .as_ref()
        .is_some_and(|fq| fq.r_squared < 0.85);

    // Trigger B: selected strength near boundary
    let s = diag.selected_strength;
    let range = s_max - s_min;
    let near_boundary = range > 0.0
        && (s - s_min < range * 0.10 || s_max - s < range * 0.10);

    if !poor_fit && !near_boundary {
        return;
    }

    let severity = if poor_fit {
        Severity::Warning
    } else {
        Severity::Suggestion
    };
    let confidence = if poor_fit {
        let r2 = diag.fit_quality.as_ref().unwrap().r_squared;
        ((0.85 - r2).min(0.3) / 0.3).clamp(0.0, 1.0) as f32
    } else {
        0.6
    };

    let reason = if poor_fit {
        let r2 = diag.fit_quality.as_ref().unwrap().r_squared;
        format!(
            "Curve fit quality is low (R\u{00b2}={:.2}). Wider probe coverage \
             should improve accuracy.",
            r2,
        )
    } else {
        "Selected strength is near the probe boundary. Widening the range \
         gives the solver more room."
            .into()
    };

    let new_probes = widen_probes(&diag.probe_samples, s_min, s_max);

    recs.push(Recommendation {
        kind: RecommendationKind::WidenProbeRange,
        severity,
        confidence,
        reason,
        patch: ParamPatch {
            probe_strengths: Some(ProbeConfig::Explicit(new_probes)),
            ..ParamPatch::default()
        },
    });
}

// ---------------------------------------------------------------------------
// Rule 6 — LowerSigma
// ---------------------------------------------------------------------------

fn rule_lower_sigma(
    diag: &AutoSharpDiagnostics,
    params: &AutoSharpParams,
    recs: &mut Vec<Recommendation>,
) {
    let features = match diag.evaluator_result.as_ref() {
        Some(e) => &e.features,
        None => return,
    };
    if features.edge_density <= 0.25 || params.sharpen_sigma <= 1.5 {
        return;
    }

    let confidence = ((features.edge_density - 0.25).min(0.15) / 0.15).clamp(0.0, 1.0);
    let new_sigma = (params.sharpen_sigma * 0.6 * 10.0).round() / 10.0;

    recs.push(Recommendation {
        kind: RecommendationKind::LowerSigma,
        severity: Severity::Info,
        confidence,
        reason: format!(
            "This image is detail-rich (edge density {:.0}%). A lower sigma \
             sharpens finer features with less halo risk.",
            features.edge_density * 100.0,
        ),
        patch: ParamPatch {
            sharpen_sigma: Some(new_sigma),
            ..ParamPatch::default()
        },
    });
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Get (min, max) strength from probe samples.
fn probe_range(probes: &[crate::types::ProbeSample]) -> (f32, f32) {
    if probes.is_empty() {
        return (0.0, 0.0);
    }
    let mut s_min = f32::INFINITY;
    let mut s_max = f32::NEG_INFINITY;
    for p in probes {
        s_min = s_min.min(p.strength);
        s_max = s_max.max(p.strength);
    }
    (s_min, s_max)
}

/// Linear-interpolate metric_value at `s` from the two nearest probe samples.
fn interpolate_from_probes(probes: &[crate::types::ProbeSample], s: f32) -> Option<f32> {
    if probes.len() < 2 {
        return None;
    }
    let mut sorted: Vec<_> = probes.iter().collect();
    sorted.sort_by(|a, b| a.strength.partial_cmp(&b.strength).unwrap());

    // Find the interval containing s.
    for window in sorted.windows(2) {
        let (a, b) = (window[0], window[1]);
        if a.strength <= s && s <= b.strength {
            let span = b.strength - a.strength;
            if span < 1e-9 {
                return Some(a.metric_value);
            }
            let t = (s - a.strength) / span;
            return Some(a.metric_value + t * (b.metric_value - a.metric_value));
        }
    }
    None
}

/// Widen probe range by ~50% in each direction, adding intermediate points.
fn widen_probes(
    probes: &[crate::types::ProbeSample],
    s_min: f32,
    s_max: f32,
) -> Vec<f32> {
    let mut strengths: Vec<f32> = probes.iter().map(|p| p.strength).collect();

    let extent = s_max - s_min;
    let low_ext = (extent * 0.5).max(0.02);
    let high_ext = (extent * 0.5).max(0.02);

    let new_low = (s_min - low_ext).max(0.01);
    let new_high = (s_max + high_ext).min(3.0);

    // Add two evenly-spaced points in the low extension.
    let low_step = (s_min - new_low) / 3.0;
    if low_step > 0.005 {
        strengths.push(new_low + low_step);
        strengths.push(new_low + 2.0 * low_step);
    }
    strengths.push(new_low);

    // Add two evenly-spaced points in the high extension.
    let high_step = (new_high - s_max) / 3.0;
    if high_step > 0.005 {
        strengths.push(s_max + high_step);
        strengths.push(s_max + 2.0 * high_step);
    }
    strengths.push(new_high);

    // Sort and dedup.
    strengths.sort_by(|a, b| a.partial_cmp(b).unwrap());
    strengths.dedup_by(|a, b| (*a - *b).abs() < 0.005);

    strengths
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    /// Minimal diagnostics for testing — most fields at their simplest state.
    fn base_diag() -> AutoSharpDiagnostics {
        AutoSharpDiagnostics {
            input_size: ImageSize { width: 100, height: 100 },
            output_size: ImageSize { width: 50, height: 50 },
            sharpen_mode: SharpenMode::Lightness,
            metric_mode: MetricMode::RelativeToBase,
            artifact_metric: ArtifactMetric::ChannelClippingRatio,
            target_artifact_ratio: 0.001,
            baseline_artifact_ratio: 0.0,
            probe_samples: vec![
                ProbeSample { strength: 0.05, artifact_ratio: 0.0001, metric_value: 0.0001, breakdown: None },
                ProbeSample { strength: 0.15, artifact_ratio: 0.0005, metric_value: 0.0005, breakdown: None },
                ProbeSample { strength: 0.30, artifact_ratio: 0.0015, metric_value: 0.0015, breakdown: None },
                ProbeSample { strength: 0.50, artifact_ratio: 0.0040, metric_value: 0.0040, breakdown: None },
            ],
            fit_status: FitStatus::Success,
            fit_coefficients: Some(CubicPolynomial { a: 0.01, b: 0.005, c: 0.001, d: 0.0 }),
            fit_quality: Some(FitQuality {
                residual_sum_of_squares: 1e-8,
                r_squared: 0.99,
                max_residual: 1e-4,
                min_pivot: 0.5,
            }),
            crossing_status: CrossingStatus::Found,
            robustness: None,
            selected_strength: 0.30,
            selection_mode: SelectionMode::PolynomialRoot,
            fallback_reason: None,
            budget_reachable: true,
            measured_artifact_ratio: 0.001,
            measured_metric_value: 0.001,
            metric_components: None,
            metric_weights: MetricWeights::default(),
            region_coverage: None,
            adaptive_validation: None,
            timing: StageTiming::default(),
            input_ingress: None,
            resize_strategy_diagnostics: None,
            chroma_guard: None,
            evaluator_result: None,
            recommendations: Vec::new(),
            probe_pass_diagnostics: None,
        }
    }

    fn base_params() -> AutoSharpParams {
        AutoSharpParams::default()
    }

    fn eval_with_edge_density(edge_density: f32) -> QualityEvaluation {
        QualityEvaluation {
            predicted_quality_score: 0.8,
            suggested_strength: Some(0.25),
            confidence: 0.7,
            features: ImageFeatures {
                edge_density,
                mean_gradient_magnitude: 0.1,
                gradient_variance: 0.01,
                mean_local_variance: 0.01,
                local_variance_variance: 0.001,
                laplacian_variance: 0.01,
                luminance_histogram_entropy: 4.0,
            },
        }
    }

    // ── Rule 1: SwitchToContentAdaptive ──────────────────────────────

    #[test]
    fn rule1_fires_on_high_edge_density_uniform() {
        let mut diag = base_diag();
        diag.evaluator_result = Some(eval_with_edge_density(0.25));
        let params = base_params();

        let recs = generate_recommendations(&diag, &params);
        assert!(recs.iter().any(|r| r.kind == RecommendationKind::SwitchToContentAdaptive));
        let rec = recs.iter().find(|r| r.kind == RecommendationKind::SwitchToContentAdaptive).unwrap();
        assert!(rec.patch.sharpen_strategy.is_some());
    }

    #[test]
    fn rule1_skips_when_already_adaptive() {
        let mut diag = base_diag();
        diag.evaluator_result = Some(eval_with_edge_density(0.25));
        let mut params = base_params();
        params.sharpen_strategy = SharpenStrategy::ContentAdaptive {
            classification: ClassificationParams::default(),
            gain_table: GainTable::v03_default(),
            max_backoff_iterations: 4,
            backoff_scale_factor: 0.8,
        };

        let recs = generate_recommendations(&diag, &params);
        assert!(!recs.iter().any(|r| r.kind == RecommendationKind::SwitchToContentAdaptive));
    }

    #[test]
    fn rule1_skips_when_low_edge_density() {
        let mut diag = base_diag();
        diag.evaluator_result = Some(eval_with_edge_density(0.10));

        let recs = generate_recommendations(&diag, &base_params());
        assert!(!recs.iter().any(|r| r.kind == RecommendationKind::SwitchToContentAdaptive));
    }

    // ── Rule 2: LowerStrongEdgeGain ──────────────────────────────────

    #[test]
    fn rule2_fires_on_high_halo_zone_fraction() {
        let mut diag = base_diag();
        diag.region_coverage = Some(RegionCoverage {
            total_pixels: 10000,
            flat: 5000, textured: 2000, strong_edge: 1000,
            microtexture: 500, risky_halo_zone: 2000,
            flat_fraction: 0.50, textured_fraction: 0.20,
            strong_edge_fraction: 0.10, microtexture_fraction: 0.05,
            risky_halo_zone_fraction: 0.20,
        });
        let mut params = base_params();
        params.sharpen_strategy = SharpenStrategy::ContentAdaptive {
            classification: ClassificationParams::default(),
            gain_table: GainTable::v03_default(),
            max_backoff_iterations: 4,
            backoff_scale_factor: 0.8,
        };

        let recs = generate_recommendations(&diag, &params);
        assert!(recs.iter().any(|r| r.kind == RecommendationKind::LowerStrongEdgeGain));
    }

    #[test]
    fn rule2_skips_when_uniform() {
        let mut diag = base_diag();
        diag.region_coverage = Some(RegionCoverage {
            total_pixels: 10000,
            flat: 5000, textured: 2000, strong_edge: 1000,
            microtexture: 500, risky_halo_zone: 2000,
            flat_fraction: 0.50, textured_fraction: 0.20,
            strong_edge_fraction: 0.10, microtexture_fraction: 0.05,
            risky_halo_zone_fraction: 0.20,
        });

        let recs = generate_recommendations(&diag, &base_params());
        assert!(!recs.iter().any(|r| r.kind == RecommendationKind::LowerStrongEdgeGain));
    }

    // ── Rule 3: RaiseArtifactBudget ──────────────────────────────────

    #[test]
    fn rule3_fires_when_evaluator_suggests_higher_strength() {
        let mut diag = base_diag();
        diag.selected_strength = 0.15;
        // Use steeper coefficients so P_hat(0.40) >> default target 0.001.
        diag.fit_coefficients = Some(CubicPolynomial { a: 0.5, b: 0.1, c: 0.01, d: 0.0 });
        let mut eval = eval_with_edge_density(0.10);
        eval.suggested_strength = Some(0.40); // > 30% diff from 0.15, within probe range
        diag.evaluator_result = Some(eval);

        let recs = generate_recommendations(&diag, &base_params());
        assert!(recs.iter().any(|r| r.kind == RecommendationKind::RaiseArtifactBudget));
        let rec = recs.iter().find(|r| r.kind == RecommendationKind::RaiseArtifactBudget).unwrap();
        assert!(rec.patch.target_artifact_ratio.is_some());
    }

    #[test]
    fn rule3_skips_when_close_to_current() {
        let mut diag = base_diag();
        diag.selected_strength = 0.30;
        let mut eval = eval_with_edge_density(0.10);
        eval.suggested_strength = Some(0.32); // within 30%
        diag.evaluator_result = Some(eval);

        let recs = generate_recommendations(&diag, &base_params());
        assert!(!recs.iter().any(|r| r.kind == RecommendationKind::RaiseArtifactBudget));
    }

    // ── Rule 4: SwitchToLightness ────────────────────────────────────

    #[test]
    fn rule4_fires_on_rgb_mode_with_edges() {
        let mut diag = base_diag();
        diag.evaluator_result = Some(eval_with_edge_density(0.20));
        let mut params = base_params();
        params.sharpen_mode = SharpenMode::Rgb;

        let recs = generate_recommendations(&diag, &params);
        assert!(recs.iter().any(|r| r.kind == RecommendationKind::SwitchToLightness));
    }

    #[test]
    fn rule4_skips_when_already_lightness() {
        let mut diag = base_diag();
        diag.evaluator_result = Some(eval_with_edge_density(0.20));

        let recs = generate_recommendations(&diag, &base_params());
        assert!(!recs.iter().any(|r| r.kind == RecommendationKind::SwitchToLightness));
    }

    // ── Rule 5: WidenProbeRange ──────────────────────────────────────

    #[test]
    fn rule5_fires_on_poor_r_squared() {
        let mut diag = base_diag();
        diag.fit_quality = Some(FitQuality {
            residual_sum_of_squares: 0.01,
            r_squared: 0.60,
            max_residual: 0.05,
            min_pivot: 0.1,
        });

        let recs = generate_recommendations(&diag, &base_params());
        assert!(recs.iter().any(|r| r.kind == RecommendationKind::WidenProbeRange));
        let rec = recs.iter().find(|r| r.kind == RecommendationKind::WidenProbeRange).unwrap();
        assert!(matches!(rec.severity, Severity::Warning));
    }

    #[test]
    fn rule5_fires_when_strength_near_boundary() {
        let mut diag = base_diag();
        diag.selected_strength = 0.052; // near min probe 0.05

        let recs = generate_recommendations(&diag, &base_params());
        assert!(recs.iter().any(|r| r.kind == RecommendationKind::WidenProbeRange));
        let rec = recs.iter().find(|r| r.kind == RecommendationKind::WidenProbeRange).unwrap();
        assert!(matches!(rec.severity, Severity::Suggestion));
    }

    #[test]
    fn rule5_skips_when_good_fit_and_centered() {
        let diag = base_diag(); // r² = 0.99, strength = 0.30 (centered)
        let recs = generate_recommendations(&diag, &base_params());
        assert!(!recs.iter().any(|r| r.kind == RecommendationKind::WidenProbeRange));
    }

    // ── Rule 6: LowerSigma ───────────────────────────────────────────

    #[test]
    fn rule6_fires_on_detail_rich_with_high_sigma() {
        let mut diag = base_diag();
        diag.evaluator_result = Some(eval_with_edge_density(0.35));
        let mut params = base_params();
        params.sharpen_sigma = 2.0;

        let recs = generate_recommendations(&diag, &params);
        assert!(recs.iter().any(|r| r.kind == RecommendationKind::LowerSigma));
        let rec = recs.iter().find(|r| r.kind == RecommendationKind::LowerSigma).unwrap();
        let new_sigma = rec.patch.sharpen_sigma.unwrap();
        assert!((new_sigma - 1.2).abs() < 0.05); // 2.0 * 0.6 = 1.2
    }

    #[test]
    fn rule6_skips_when_sigma_already_low() {
        let mut diag = base_diag();
        diag.evaluator_result = Some(eval_with_edge_density(0.35));
        let mut params = base_params();
        params.sharpen_sigma = 1.0;

        let recs = generate_recommendations(&diag, &params);
        assert!(!recs.iter().any(|r| r.kind == RecommendationKind::LowerSigma));
    }

    // ── Invariant: no field overlap ──────────────────────────────────

    #[test]
    fn no_two_recommendations_share_a_field() {
        // Set up a scenario that fires multiple rules.
        let mut diag = base_diag();
        diag.evaluator_result = Some(eval_with_edge_density(0.30));
        diag.fit_quality = Some(FitQuality {
            residual_sum_of_squares: 0.01,
            r_squared: 0.50,
            max_residual: 0.05,
            min_pivot: 0.1,
        });
        let mut params = base_params();
        params.sharpen_mode = SharpenMode::Rgb;
        params.sharpen_sigma = 2.0;

        let recs = generate_recommendations(&diag, &params);

        // Check no two recs touch the same field.
        for (i, a) in recs.iter().enumerate() {
            for b in recs.iter().skip(i + 1) {
                assert!(
                    !(a.patch.sharpen_strategy.is_some() && b.patch.sharpen_strategy.is_some()),
                    "two recs both set sharpen_strategy: {:?} and {:?}", a.kind, b.kind
                );
                assert!(
                    !(a.patch.target_artifact_ratio.is_some() && b.patch.target_artifact_ratio.is_some()),
                    "two recs both set target_artifact_ratio: {:?} and {:?}", a.kind, b.kind
                );
                assert!(
                    !(a.patch.sharpen_mode.is_some() && b.patch.sharpen_mode.is_some()),
                    "two recs both set sharpen_mode: {:?} and {:?}", a.kind, b.kind
                );
                assert!(
                    !(a.patch.probe_strengths.is_some() && b.patch.probe_strengths.is_some()),
                    "two recs both set probe_strengths: {:?} and {:?}", a.kind, b.kind
                );
                assert!(
                    !(a.patch.sharpen_sigma.is_some() && b.patch.sharpen_sigma.is_some()),
                    "two recs both set sharpen_sigma: {:?} and {:?}", a.kind, b.kind
                );
            }
        }
    }

    // ── Helpers ──────────────────────────────────────────────────────

    #[test]
    fn widen_probes_produces_sorted_range() {
        let probes = vec![
            ProbeSample { strength: 0.05, artifact_ratio: 0.0, metric_value: 0.0, breakdown: None },
            ProbeSample { strength: 0.50, artifact_ratio: 0.0, metric_value: 0.0, breakdown: None },
        ];
        let result = widen_probes(&probes, 0.05, 0.50);
        assert!(result.len() > 2);
        for w in result.windows(2) {
            assert!(w[0] < w[1], "not sorted: {} >= {}", w[0], w[1]);
        }
        assert!(*result.first().unwrap() < 0.05);
        assert!(*result.last().unwrap() > 0.50);
    }

    #[test]
    fn interpolate_within_range() {
        let probes = vec![
            ProbeSample { strength: 0.1, artifact_ratio: 0.0, metric_value: 0.001, breakdown: None },
            ProbeSample { strength: 0.5, artifact_ratio: 0.0, metric_value: 0.005, breakdown: None },
        ];
        let v = interpolate_from_probes(&probes, 0.3).unwrap();
        assert!((v - 0.003).abs() < 0.001);
    }

    #[test]
    fn interpolate_outside_range_returns_none() {
        let probes = vec![
            ProbeSample { strength: 0.1, artifact_ratio: 0.0, metric_value: 0.001, breakdown: None },
            ProbeSample { strength: 0.5, artifact_ratio: 0.0, metric_value: 0.005, breakdown: None },
        ];
        assert!(interpolate_from_probes(&probes, 0.6).is_none());
    }
}
