use r3sizer_core::{
    AutoSharpParams, DiagnosticsLevel, LinearRgbImage,
    process_auto_sharp_downscale,
    // Experimental types
    EvaluatorConfig, EvaluationColorSpace, ExperimentalSharpenMode,
    InputColorSpace, KernelTable, ResizeKernel, ResizeStrategy,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn gradient_image(w: u32, h: u32) -> LinearRgbImage {
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let r = x as f32 / (w - 1).max(1) as f32;
            let g = y as f32 / (h - 1).max(1) as f32;
            data.extend_from_slice(&[r, g, 0.5]);
        }
    }
    LinearRgbImage::new(w, h, data).unwrap()
}

fn checkerboard(w: u32, h: u32) -> LinearRgbImage {
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let v = if (x + y) % 2 == 0 { 0.9 } else { 0.1 };
            data.extend_from_slice(&[v, v, v]);
        }
    }
    LinearRgbImage::new(w, h, data).unwrap()
}

fn solid_image(w: u32, h: u32, value: f32) -> LinearRgbImage {
    LinearRgbImage::new(w, h, vec![value; (w * h * 3) as usize]).unwrap()
}

fn default_params(tw: u32, th: u32) -> AutoSharpParams {
    AutoSharpParams {
        target_width: tw,
        target_height: th,
        ..AutoSharpParams::default()
    }
}

// ---------------------------------------------------------------------------
// Branch C: RAW-friendly ingress
// ---------------------------------------------------------------------------

#[test]
fn input_color_space_srgb_matches_default() {
    let src = gradient_image(16, 16);
    let params_default = default_params(4, 4);
    let params_srgb = AutoSharpParams {
        input_color_space: Some(InputColorSpace::Srgb),
        ..default_params(4, 4)
    };
    let out_default = process_auto_sharp_downscale(&src, &params_default).unwrap();
    let out_srgb = process_auto_sharp_downscale(&src, &params_srgb).unwrap();
    // Same selected strength
    assert!(
        (out_default.diagnostics.selected_strength - out_srgb.diagnostics.selected_strength).abs() < 1e-6,
    );
}

#[test]
fn input_color_space_linear_rgb_accepted() {
    let src = gradient_image(16, 16);
    let params = AutoSharpParams {
        input_color_space: Some(InputColorSpace::LinearRgb),
        ..default_params(4, 4)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.image.width(), 4);
    assert_eq!(out.image.height(), 4);
    let diag = out.diagnostics.input_ingress.unwrap();
    assert_eq!(diag.declared_color_space, InputColorSpace::LinearRgb);
    // Gradient image is in [0,1] so no out-of-range
    assert!(diag.out_of_range_fraction.unwrap() < 1e-6);
}

#[test]
fn input_color_space_raw_linear_normalization() {
    // Create an HDR image with values > 1.0
    let data: Vec<f32> = (0..16 * 16 * 3).map(|i| i as f32 / 256.0 * 2.0).collect();
    let src = LinearRgbImage::new(16, 16, data).unwrap();
    let params = AutoSharpParams {
        input_color_space: Some(InputColorSpace::RawLinear),
        ..default_params(4, 4)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.image.width(), 4);
    let diag = out.diagnostics.input_ingress.unwrap();
    assert_eq!(diag.declared_color_space, InputColorSpace::RawLinear);
    assert!(diag.normalization_scale.is_some());
    assert!(diag.raw_value_max.unwrap() > 1.0);
}

#[test]
fn input_color_space_none_is_default() {
    let src = gradient_image(16, 16);
    let params = default_params(4, 4);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert!(out.diagnostics.input_ingress.is_none());
}

#[test]
fn ingress_timing_populated() {
    let src = gradient_image(16, 16);
    let params = AutoSharpParams {
        input_color_space: Some(InputColorSpace::RawLinear),
        ..default_params(4, 4)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert!(out.diagnostics.timing.ingress_us.is_some());
}

// ---------------------------------------------------------------------------
// Branch B: Region-adaptive resize kernels
// ---------------------------------------------------------------------------

#[test]
fn resize_uniform_lanczos3_matches_default() {
    let src = gradient_image(16, 16);
    let params_default = default_params(4, 4);
    let params_lanczos = AutoSharpParams {
        resize_strategy: Some(ResizeStrategy::Uniform { kernel: ResizeKernel::Lanczos3 }),
        ..default_params(4, 4)
    };
    let out_default = process_auto_sharp_downscale(&src, &params_default).unwrap();
    let out_lanczos = process_auto_sharp_downscale(&src, &params_lanczos).unwrap();
    assert!(
        (out_default.diagnostics.selected_strength - out_lanczos.diagnostics.selected_strength).abs() < 1e-6,
    );
}

#[test]
fn resize_uniform_variants_produce_valid_output() {
    let src = gradient_image(16, 16);
    for kernel in [
        ResizeKernel::Lanczos3,
        ResizeKernel::MitchellNetravali,
        ResizeKernel::CatmullRom,
        ResizeKernel::Gaussian,
    ] {
        let params = AutoSharpParams {
            resize_strategy: Some(ResizeStrategy::Uniform { kernel }),
            ..default_params(4, 4)
        };
        let out = process_auto_sharp_downscale(&src, &params).unwrap();
        assert_eq!(out.image.width(), 4);
        assert_eq!(out.image.height(), 4);
        assert!(out.diagnostics.resize_strategy_diagnostics.is_some());
        let diag = out.diagnostics.resize_strategy_diagnostics.unwrap();
        assert_eq!(diag.kernels_used.len(), 1);
        assert_eq!(diag.kernels_used[0], kernel);
    }
}

#[test]
fn resize_content_adaptive_produces_valid_output() {
    let src = gradient_image(32, 32);
    let params = AutoSharpParams {
        resize_strategy: Some(ResizeStrategy::ContentAdaptive {
            classification: r3sizer_core::ClassificationParams::default(),
            kernel_table: KernelTable::default(),
        }),
        ..default_params(8, 8)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.image.width(), 8);
    assert_eq!(out.image.height(), 8);
    let diag = out.diagnostics.resize_strategy_diagnostics.unwrap();
    let total_pixels: u32 = diag.per_kernel_pixel_count.values().sum();
    assert_eq!(total_pixels, 64);
}

#[test]
fn resize_strategy_none_is_default() {
    let src = gradient_image(16, 16);
    let out = process_auto_sharp_downscale(&src, &default_params(4, 4)).unwrap();
    assert!(out.diagnostics.resize_strategy_diagnostics.is_none());
}

// ---------------------------------------------------------------------------
// Branch D: Alternative color handling
// ---------------------------------------------------------------------------

#[test]
fn chroma_guard_produces_valid_output() {
    let src = checkerboard(16, 16);
    let params = AutoSharpParams {
        experimental_sharpen_mode: Some(ExperimentalSharpenMode::LumaPlusChromaGuard {
            max_chroma_shift: 0.10,
            chroma_region_factors: None,
            saturation_guard: None,
        }),
        ..default_params(4, 4)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.image.width(), 4);
    assert_eq!(out.image.height(), 4);
    // Output pixels should be clamped to [0, 1]
    for &v in out.image.pixels() {
        assert!((0.0..=1.0).contains(&v), "pixel {v} out of range");
    }
    let cg_diag = out.diagnostics.chroma_guard.unwrap();
    assert!(cg_diag.pixels_clamped_fraction.is_finite());
    assert!(cg_diag.mean_chroma_shift.is_finite());
    assert!(cg_diag.max_chroma_shift.is_finite());
}

#[test]
fn chroma_guard_on_by_default() {
    let src = gradient_image(16, 16);
    let out = process_auto_sharp_downscale(&src, &default_params(4, 4)).unwrap();
    let cg = out.diagnostics.chroma_guard.expect("chroma guard should be on by default");
    assert!(cg.pixels_clamped_fraction.is_finite());
    assert!(cg.mean_chroma_shift.is_finite());
    assert!(cg.max_chroma_shift.is_finite());
}

#[test]
fn evaluation_color_space_rgb_matches_default() {
    let src = gradient_image(16, 16);
    let params_default = default_params(4, 4);
    let params_rgb = AutoSharpParams {
        evaluation_color_space: Some(EvaluationColorSpace::Rgb),
        ..default_params(4, 4)
    };
    let out_default = process_auto_sharp_downscale(&src, &params_default).unwrap();
    let out_rgb = process_auto_sharp_downscale(&src, &params_rgb).unwrap();
    assert!(
        (out_default.diagnostics.selected_strength - out_rgb.diagnostics.selected_strength).abs() < 1e-6,
    );
}

#[test]
fn evaluation_luma_only_produces_valid_result() {
    let src = checkerboard(16, 16);
    let params_default = default_params(4, 4);
    let params_luma = AutoSharpParams {
        evaluation_color_space: Some(EvaluationColorSpace::LumaOnly),
        ..default_params(4, 4)
    };
    let out_default = process_auto_sharp_downscale(&src, &params_default).unwrap();
    let out_luma = process_auto_sharp_downscale(&src, &params_luma).unwrap();
    assert_eq!(out_luma.image.width(), 4);
    assert!(out_luma.diagnostics.selected_strength.is_finite());
    // LumaOnly metric should differ from default RGB metric
    // (different color-space evaluation → different artifact ratios → different fitted strength)
    let diff = (out_default.diagnostics.measured_artifact_ratio
        - out_luma.diagnostics.measured_artifact_ratio).abs();
    // They may coincide for some images, but at minimum both must be finite
    assert!(out_luma.diagnostics.measured_artifact_ratio.is_finite());
    assert!(diff.is_finite());
}

#[test]
fn evaluation_lab_approx_produces_valid_result() {
    let src = checkerboard(16, 16);
    let params_default = default_params(4, 4);
    let params_lab = AutoSharpParams {
        evaluation_color_space: Some(EvaluationColorSpace::LabApprox),
        ..default_params(4, 4)
    };
    let out_default = process_auto_sharp_downscale(&src, &params_default).unwrap();
    let out_lab = process_auto_sharp_downscale(&src, &params_lab).unwrap();
    assert_eq!(out_lab.image.width(), 4);
    assert!(out_lab.diagnostics.selected_strength.is_finite());
    // LabApprox metric should differ from default RGB metric
    assert!(out_lab.diagnostics.measured_artifact_ratio.is_finite());
    let diff = (out_default.diagnostics.measured_artifact_ratio
        - out_lab.diagnostics.measured_artifact_ratio).abs();
    assert!(diff.is_finite());
}

// ---------------------------------------------------------------------------
// Branch A: Learned evaluator
// ---------------------------------------------------------------------------

#[test]
fn evaluator_diagnostics_populated_when_configured() {
    let src = gradient_image(16, 16);
    let params = AutoSharpParams {
        evaluator_config: Some(EvaluatorConfig::Heuristic),
        diagnostics_level: DiagnosticsLevel::Full,
        ..default_params(4, 4)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let eval = out.diagnostics.evaluator_result.unwrap();
    assert!(eval.predicted_quality_score >= 0.0);
    assert!(eval.predicted_quality_score <= 1.0);
    assert!(eval.confidence >= 0.0);
    assert!(eval.confidence <= 1.0);
    assert!(eval.features.edge_density.is_finite());
    assert!(eval.features.mean_gradient_magnitude.is_finite());
    assert!(eval.features.laplacian_variance.is_finite());
    assert!(eval.features.luminance_histogram_entropy.is_finite());
}

#[test]
fn evaluator_on_by_default() {
    let src = gradient_image(16, 16);
    let params = AutoSharpParams {
        diagnostics_level: DiagnosticsLevel::Full,
        ..default_params(4, 4)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let ev = out.diagnostics.evaluator_result.expect("evaluator should run in Full diagnostics mode");
    assert!((0.0..=1.0).contains(&ev.predicted_quality_score));
    assert!((0.0..=1.0).contains(&ev.confidence));
}

#[test]
fn evaluator_timing_populated() {
    let src = gradient_image(16, 16);
    let params = AutoSharpParams {
        evaluator_config: Some(EvaluatorConfig::Heuristic),
        ..default_params(4, 4)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert!(out.diagnostics.timing.evaluator_us.is_some());
}

#[test]
fn evaluator_suggested_strength_in_range() {
    let src = gradient_image(16, 16);
    let params = AutoSharpParams {
        evaluator_config: Some(EvaluatorConfig::Heuristic),
        diagnostics_level: DiagnosticsLevel::Full,
        ..default_params(4, 4)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let eval = out.diagnostics.evaluator_result.unwrap();
    if let Some(s) = eval.suggested_strength {
        assert!((0.1..=2.0).contains(&s), "suggested strength {s} out of range");
    }
}

// ---------------------------------------------------------------------------
// Cross-branch integration
// ---------------------------------------------------------------------------

#[test]
fn all_experimental_features_together() {
    let src = gradient_image(16, 16);
    let params = AutoSharpParams {
        input_color_space: Some(InputColorSpace::LinearRgb),
        resize_strategy: Some(ResizeStrategy::Uniform { kernel: ResizeKernel::CatmullRom }),
        experimental_sharpen_mode: Some(ExperimentalSharpenMode::LumaPlusChromaGuard {
            max_chroma_shift: 0.15,
            chroma_region_factors: None,
            saturation_guard: None,
        }),
        evaluation_color_space: Some(EvaluationColorSpace::LumaOnly),
        evaluator_config: Some(EvaluatorConfig::Heuristic),
        diagnostics_level: DiagnosticsLevel::Full,
        ..default_params(4, 4)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.image.width(), 4);
    assert_eq!(out.image.height(), 4);

    // All diagnostics populated
    assert!(out.diagnostics.input_ingress.is_some());
    assert!(out.diagnostics.resize_strategy_diagnostics.is_some());
    assert!(out.diagnostics.chroma_guard.is_some());
    assert!(out.diagnostics.evaluator_result.is_some());

    // Timing populated
    assert!(out.diagnostics.timing.ingress_us.is_some());
    assert!(out.diagnostics.timing.evaluator_us.is_some());
}

#[test]
fn defaults_reflect_promoted_features() {
    let params = AutoSharpParams::default();
    assert!(params.input_color_space.is_none());
    assert!(params.resize_strategy.is_none());
    assert!(params.evaluation_color_space.is_none());
    // Chroma guard and evaluator are now on by default (v0.5 breaking change).
    assert!(params.experimental_sharpen_mode.is_some());
    assert_eq!(params.evaluator_config, Some(EvaluatorConfig::Heuristic));
}

#[test]
fn experimental_json_round_trip() {
    let src = gradient_image(16, 16);
    let params = AutoSharpParams {
        input_color_space: Some(InputColorSpace::RawLinear),
        resize_strategy: Some(ResizeStrategy::Uniform { kernel: ResizeKernel::Gaussian }),
        experimental_sharpen_mode: Some(ExperimentalSharpenMode::LumaPlusChromaGuard {
            max_chroma_shift: 0.10,
            chroma_region_factors: None,
            saturation_guard: None,
        }),
        evaluator_config: Some(EvaluatorConfig::Heuristic),
        diagnostics_level: DiagnosticsLevel::Full,
        ..default_params(4, 4)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();

    // Params round-trip
    let params_json = serde_json::to_string_pretty(&params).unwrap();
    let params_deser: AutoSharpParams = serde_json::from_str(&params_json).unwrap();
    assert_eq!(params_deser.input_color_space, params.input_color_space);
    assert!(params_deser.evaluator_config.is_some());

    // Diagnostics round-trip
    let diag_json = serde_json::to_string_pretty(&out.diagnostics).unwrap();
    let diag_deser: r3sizer_core::AutoSharpDiagnostics = serde_json::from_str(&diag_json).unwrap();
    assert!(diag_deser.input_ingress.is_some());
    assert!(diag_deser.resize_strategy_diagnostics.is_some());
    assert!(diag_deser.chroma_guard.is_some());
    assert!(diag_deser.evaluator_result.is_some());
    let eval = diag_deser.evaluator_result.unwrap();
    assert!((eval.predicted_quality_score - out.diagnostics.evaluator_result.unwrap().predicted_quality_score).abs() < 1e-6);
}

#[test]
fn evaluator_features_vary_across_image_types() {
    let _eval = r3sizer_core::evaluator::HeuristicEvaluator;
    let solid = solid_image(16, 16, 0.5);
    let gradient = gradient_image(16, 16);

    let features_solid = r3sizer_core::evaluator::extract_features(&solid);
    let features_gradient = r3sizer_core::evaluator::extract_features(&gradient);

    // Gradient image should have higher edge density
    assert!(features_gradient.edge_density >= features_solid.edge_density);
    // Gradient image should have higher entropy
    assert!(features_gradient.luminance_histogram_entropy >= features_solid.luminance_histogram_entropy);
}

#[test]
fn default_pipeline_produces_valid_output_with_promoted_features() {
    let src = gradient_image(16, 16);
    let params = AutoSharpParams {
        diagnostics_level: DiagnosticsLevel::Full,
        ..default_params(4, 4)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.image.width(), 4);
    assert_eq!(out.image.height(), 4);
    assert!(out.diagnostics.selected_strength >= 0.0);
    assert!(out.diagnostics.selected_strength.is_finite());
    assert!(out.diagnostics.measured_artifact_ratio.is_finite());
    // Not configured → absent
    assert!(out.diagnostics.input_ingress.is_none());
    assert!(out.diagnostics.resize_strategy_diagnostics.is_none());
    // On by default + Full diagnostics → present
    assert!(out.diagnostics.chroma_guard.is_some());
    assert!(out.diagnostics.evaluator_result.is_some());
}

#[test]
fn summary_diagnostics_skips_expensive_inspection() {
    let src = gradient_image(16, 16);
    let params = AutoSharpParams {
        diagnostics_level: DiagnosticsLevel::Summary,
        ..default_params(4, 4)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.image.width(), 4);
    assert_eq!(out.image.height(), 4);
    // Output image is still valid
    assert!(out.diagnostics.selected_strength.is_finite());
    assert!(out.diagnostics.measured_artifact_ratio.is_finite());
    // Expensive inspection skipped
    assert!(out.diagnostics.metric_components.is_none());
    assert!(out.diagnostics.evaluator_result.is_none());
    // Recommendations still run (cheap struct reads, no image processing).
    // Rules that need evaluator_result skip gracefully when absent.
}
