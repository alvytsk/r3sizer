//! Generates TypeScript type definitions from Rust types using ts-rs.
//!
//! Run with:
//!   cargo test -p r3sizer-core --features typegen export_typescript_bindings -- --nocapture
//!
//! Output: web/src/types/generated.ts

#![cfg(feature = "typegen")]

use ts_rs::{Config, TS};

#[allow(deprecated)] // MetricBreakdown.aggregate
use r3sizer_core::{
    AdaptiveValidationOutcome, ArtifactMetric, AutoSharpDiagnostics, AutoSharpParams,
    ChromaGuardDiagnostics, ClampPolicy, ClassificationParams, CrossingStatus, CubicPolynomial,
    DiagnosticsLevel, EvaluationColorSpace, EvaluatorConfig, ExperimentalSharpenMode,
    FallbackReason, FitQuality, FitStatus, FitStrategy, GainTable, ImageFeatures, ImageSize,
    InputColorSpace, InputIngressDiagnostics, KernelTable, MetricBreakdown, MetricComponent,
    MetricMode, MetricWeights, ProbeConfig, ProbeSample, QualityEvaluation,
    RegionClass, RegionCoverage, ResizeKernel, ResizeStrategy, ResizeStrategyDiagnostics,
    RobustnessFlags, SelectionMode, SharpenMode, SharpenStrategy, StageTiming,
};

#[test]
fn export_typescript_bindings() {
    // u64 timing fields should be `number`, not `bigint` (JS numbers are fine for microseconds).
    let cfg = Config::new().with_large_int("number");

    let header = "\
// Auto-generated from r3sizer-core Rust types. DO NOT EDIT.
//
// Regenerate with:
//   cargo test -p r3sizer-core --features typegen export_typescript_bindings -- --nocapture
//
// Any manual TypeScript types (e.g. ProcessResult) belong in wasm-types.ts,
// which re-exports everything from this file.

/* eslint-disable */
/* prettier-ignore */
";

    // Collect declarations in dependency order (leaf types first).
    // Within a single file, TS doesn't require strict ordering, but this
    // keeps the output readable.
    #[allow(deprecated)]
    let declarations: Vec<String> = vec![
        // ── Primitives / enums ──────────────────────────────────────────
        SharpenMode::decl(&cfg),
        MetricMode::decl(&cfg),
        ArtifactMetric::decl(&cfg),
        FitStrategy::decl(&cfg),
        ClampPolicy::decl(&cfg),
        DiagnosticsLevel::decl(&cfg),
        CrossingStatus::decl(&cfg),
        SelectionMode::decl(&cfg),
        FallbackReason::decl(&cfg),
        MetricComponent::decl(&cfg),
        RegionClass::decl(&cfg),
        // ── Geometry ────────────────────────────────────────────────────
        ImageSize::decl(&cfg),
        // ── Configuration structs ───────────────────────────────────────
        MetricWeights::decl(&cfg),
        GainTable::decl(&cfg),
        ClassificationParams::decl(&cfg),
        ProbeConfig::decl(&cfg),
        SharpenStrategy::decl(&cfg),
        AutoSharpParams::decl(&cfg),
        // ── Fit / solve results ─────────────────────────────────────────
        CubicPolynomial::decl(&cfg),
        FitQuality::decl(&cfg),
        RobustnessFlags::decl(&cfg),
        FitStatus::decl(&cfg),
        // ── Metric breakdown ────────────────────────────────────────────
        MetricBreakdown::decl(&cfg),
        // ── Probe / diagnostics ─────────────────────────────────────────
        ProbeSample::decl(&cfg),
        StageTiming::decl(&cfg),
        RegionCoverage::decl(&cfg),
        AdaptiveValidationOutcome::decl(&cfg),
        AutoSharpDiagnostics::decl(&cfg),
    ];

    // Extended types (promoted from experimental in v0.5).
    let declarations = {
        let mut d = declarations;
        d.extend(vec![
            InputColorSpace::decl(&cfg),
            ResizeKernel::decl(&cfg),
            KernelTable::decl(&cfg),
            ResizeStrategy::decl(&cfg),
            ResizeStrategyDiagnostics::decl(&cfg),
            ExperimentalSharpenMode::decl(&cfg),
            EvaluationColorSpace::decl(&cfg),
            ChromaGuardDiagnostics::decl(&cfg),
            EvaluatorConfig::decl(&cfg),
            ImageFeatures::decl(&cfg),
            QualityEvaluation::decl(&cfg),
            InputIngressDiagnostics::decl(&cfg),
        ]);
        d
    };

    let mut output = String::with_capacity(8192);
    output.push_str(header);
    output.push('\n');

    for decl in &declarations {
        // ts-rs v12 omits `export` — prepend it so the web app can import these types.
        if decl.starts_with("type ") {
            output.push_str("export ");
        }
        output.push_str(decl);
        output.push_str("\n\n");
    }

    // ── Default constants (serialized from Rust Default impls) ──────────
    output.push_str("// ── Default constants (generated from Rust Default impls) ──\n\n");

    let default_weights = serde_json::to_string_pretty(&MetricWeights::default()).unwrap();
    output.push_str(&format!(
        "export const DEFAULT_METRIC_WEIGHTS: MetricWeights = {};\n\n",
        default_weights
    ));

    let default_gain_table = serde_json::to_string_pretty(&GainTable::v03_default()).unwrap();
    output.push_str(&format!(
        "export const DEFAULT_GAIN_TABLE: GainTable = {};\n\n",
        default_gain_table
    ));

    let default_classification =
        serde_json::to_string_pretty(&ClassificationParams::default()).unwrap();
    output.push_str(&format!(
        "export const DEFAULT_CLASSIFICATION_PARAMS: ClassificationParams = {};\n\n",
        default_classification
    ));

    let default_strategy = serde_json::to_string_pretty(&SharpenStrategy::default()).unwrap();
    output.push_str(&format!(
        "export const DEFAULT_SHARPEN_STRATEGY: SharpenStrategy = {};\n\n",
        default_strategy
    ));

    let default_params = serde_json::to_string_pretty(&AutoSharpParams::default()).unwrap();
    output.push_str(&format!(
        "export const DEFAULT_PARAMS: AutoSharpParams = {};\n\n",
        default_params
    ));

    let default_kernel_table =
        serde_json::to_string_pretty(&KernelTable::default()).unwrap();
    output.push_str(&format!(
        "export const DEFAULT_KERNEL_TABLE: KernelTable = {};\n",
        default_kernel_table
    ));

    // Write to web directory
    let out_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../web/src/types/generated.ts");
    std::fs::write(&out_path, &output).expect("failed to write generated.ts");

    println!("✓ Wrote {}", out_path.display());
}
