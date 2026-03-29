# Hybrid Selection Policy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `SelectionPolicy` (GamutOnly / Hybrid / CompositeOnly) so that the composite metric participates in strength selection without replacing the gamut safety constraint.

**Architecture:** Gamut excursion remains the hard budget constraint and polynomial fitting target. In Hybrid mode, the composite score drives fallback ranking (BestSampleWithinBudget picks lowest composite among safe candidates; LeastBadSample picks lowest composite overall). CompositeOnly is experimental and deferred — the enum variant exists but is treated as Hybrid for now. The polynomial root path is unchanged across all policies.

**Tech Stack:** Rust (r3sizer-core, r3sizer-cli), serde, ts-rs, clap

---

## File Map

| Action | File | Responsibility |
|--------|------|----------------|
| Modify | `crates/r3sizer-core/src/types.rs` | Add `SelectionPolicy` enum, field on `AutoSharpParams` and `AutoSharpDiagnostics` |
| Modify | `crates/r3sizer-core/src/solve.rs` | Policy-aware `fallback_from_samples` |
| Modify | `crates/r3sizer-core/src/pipeline.rs` | Wire policy through probe → solve → diagnostics |
| Modify | `crates/r3sizer-core/src/lib.rs` | Re-export `SelectionPolicy` |
| Modify | `crates/r3sizer-cli/src/args.rs` | Add `--selection-policy` CLI arg |
| Modify | `crates/r3sizer-cli/src/run.rs` | Map CLI arg into `AutoSharpParams` |
| Modify | `crates/r3sizer-core/tests/integration.rs` | Integration tests for all three policies |
| Modify | `docs/future_work.md` | Remove stale "stubs" language, document selection policy |

---

### Task 1: Add `SelectionPolicy` enum and param/diagnostics fields

**Files:**
- Modify: `crates/r3sizer-core/src/types.rs`

- [ ] **Step 1: Add the `SelectionPolicy` enum**

After the `MetricMode` enum (around line 130), add:

```rust
/// How the solver ranks candidate sharpening strengths.
///
/// Orthogonal to [`FitStrategy`] (which controls polynomial vs direct search)
/// and to [`ArtifactMetric`] (which selects the gamut measurement function).
/// `SelectionPolicy` controls how fallback candidates are ranked when the
/// polynomial root is unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[serde(rename_all = "snake_case")]
pub enum SelectionPolicy {
    /// Current behavior: gamut excursion drives both fitting and fallback ranking.
    /// Fallback picks the largest strength within budget, or the least-bad by
    /// gamut metric when budget is unreachable.
    #[default]
    GamutOnly,
    /// Gamut excursion remains the hard safety constraint and fitting target.
    /// Fallback ranking uses composite score: among budget-qualifying samples,
    /// prefer the one with the lowest composite penalty; among all samples when
    /// budget is unreachable, prefer the lowest composite penalty.
    Hybrid,
    /// Experimental: composite score drives both fitting and fallback ranking.
    /// Currently treated as Hybrid (polynomial fitting still uses gamut).
    CompositeOnly,
}
```

- [ ] **Step 2: Add `selection_policy` field to `AutoSharpParams`**

In the `AutoSharpParams` struct, after the `metric_weights` field (line 288), add:

```rust
    /// How the solver ranks fallback candidates. Default: `GamutOnly`.
    #[serde(default)]
    pub selection_policy: SelectionPolicy,
```

In the `Default` impl for `AutoSharpParams` (around line 318), add inside the struct literal:

```rust
            selection_policy: SelectionPolicy::default(),
```

- [ ] **Step 3: Add `selection_policy` field to `AutoSharpDiagnostics`**

In the `AutoSharpDiagnostics` struct, after the `artifact_metric` field (line 904), add:

```rust
    /// Which selection policy was used for fallback ranking.
    #[serde(default)]
    pub selection_policy: SelectionPolicy,
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build -p r3sizer-core 2>&1 | head -30`
Expected: compilation errors in `pipeline.rs` (missing field `selection_policy` in diagnostics struct literal) — this is expected and will be fixed in Task 3.

- [ ] **Step 5: Commit**

```bash
git add crates/r3sizer-core/src/types.rs
git commit -m "feat(types): add SelectionPolicy enum (GamutOnly, Hybrid, CompositeOnly)

Adds the selection_policy field to AutoSharpParams and AutoSharpDiagnostics.
GamutOnly is the default for backward compatibility. Hybrid uses composite
score for fallback ranking. CompositeOnly is experimental (treated as Hybrid).

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Policy-aware fallback in solve.rs

**Files:**
- Modify: `crates/r3sizer-core/src/solve.rs`

- [ ] **Step 1: Write failing unit tests for Hybrid fallback**

Add these tests at the bottom of the existing `mod tests` block in `solve.rs`:

```rust
    // -----------------------------------------------------------------------
    // SelectionPolicy tests
    // -----------------------------------------------------------------------

    use crate::{MetricBreakdown, MetricComponent, SelectionPolicy};
    use std::collections::BTreeMap;

    fn make_sample_with_composite(
        strength: f32,
        metric_value: f32,
        composite_score: f32,
    ) -> ProbeSample {
        let mut components = BTreeMap::new();
        components.insert(MetricComponent::GamutExcursion, metric_value);
        components.insert(MetricComponent::HaloRinging, 0.0);
        components.insert(MetricComponent::EdgeOvershoot, 0.0);
        components.insert(MetricComponent::TextureFlattening, 0.0);
        #[allow(deprecated)]
        let breakdown = MetricBreakdown {
            components,
            selected_metric: MetricComponent::GamutExcursion,
            selection_score: metric_value,
            composite_score,
            aggregate: metric_value,
        };
        ProbeSample {
            strength,
            artifact_ratio: metric_value,
            metric_value,
            breakdown: Some(breakdown),
        }
    }

    #[test]
    fn gamut_only_fallback_picks_max_strength_within_budget() {
        // Two samples within budget — GamutOnly should pick the one with higher strength,
        // even though it has a worse composite score.
        let samples = vec![
            make_sample_with_composite(1.0, 0.0005, 0.01),  // within budget, good composite
            make_sample_with_composite(2.0, 0.0008, 0.05),  // within budget, worse composite
            make_sample_with_composite(3.0, 0.005, 0.10),   // exceeds budget
        ];
        let result = find_sharpness_direct_with_policy(&samples, 0.001, SelectionPolicy::GamutOnly).unwrap();
        assert_eq!(result.selection_mode, SelectionMode::BestSampleWithinBudget);
        assert_abs_diff_eq!(result.selected_strength, 2.0, epsilon = 1e-5);
    }

    #[test]
    fn hybrid_fallback_picks_best_composite_within_budget() {
        // Same samples — Hybrid should pick the one with better composite,
        // even though it has lower strength.
        let samples = vec![
            make_sample_with_composite(1.0, 0.0005, 0.01),  // within budget, best composite
            make_sample_with_composite(2.0, 0.0008, 0.05),  // within budget, worse composite
            make_sample_with_composite(3.0, 0.005, 0.10),   // exceeds budget
        ];
        let result = find_sharpness_direct_with_policy(&samples, 0.001, SelectionPolicy::Hybrid).unwrap();
        assert_eq!(result.selection_mode, SelectionMode::BestSampleWithinBudget);
        assert_abs_diff_eq!(result.selected_strength, 1.0, epsilon = 1e-5);
    }

    #[test]
    fn hybrid_never_picks_out_of_budget_when_in_budget_exists() {
        // One sample in budget with poor composite, one out of budget with great composite.
        // Hybrid must still pick the in-budget one.
        let samples = vec![
            make_sample_with_composite(1.0, 0.0008, 0.09),  // within budget, poor composite
            make_sample_with_composite(2.0, 0.005, 0.001),   // exceeds budget, great composite
        ];
        let result = find_sharpness_direct_with_policy(&samples, 0.001, SelectionPolicy::Hybrid).unwrap();
        assert_eq!(result.selection_mode, SelectionMode::BestSampleWithinBudget);
        assert_abs_diff_eq!(result.selected_strength, 1.0, epsilon = 1e-5);
    }

    #[test]
    fn hybrid_least_bad_uses_composite_not_gamut() {
        // All exceed budget. GamutOnly would pick s=1.0 (lowest metric_value=0.005).
        // Hybrid should pick s=2.0 (lowest composite_score=0.02).
        let samples = vec![
            make_sample_with_composite(1.0, 0.005, 0.08),  // lowest gamut, worst composite
            make_sample_with_composite(2.0, 0.010, 0.02),  // middle gamut, best composite
            make_sample_with_composite(3.0, 0.020, 0.05),  // worst gamut, middle composite
        ];
        let result = find_sharpness_direct_with_policy(&samples, 0.001, SelectionPolicy::Hybrid).unwrap();
        assert_eq!(result.selection_mode, SelectionMode::LeastBadSample);
        assert_abs_diff_eq!(result.selected_strength, 2.0, epsilon = 1e-5);
    }

    #[test]
    fn gamut_only_least_bad_uses_gamut_metric() {
        // Same samples — GamutOnly should pick s=1.0 (lowest metric_value).
        let samples = vec![
            make_sample_with_composite(1.0, 0.005, 0.08),
            make_sample_with_composite(2.0, 0.010, 0.02),
            make_sample_with_composite(3.0, 0.020, 0.05),
        ];
        let result = find_sharpness_direct_with_policy(&samples, 0.001, SelectionPolicy::GamutOnly).unwrap();
        assert_eq!(result.selection_mode, SelectionMode::LeastBadSample);
        assert_abs_diff_eq!(result.selected_strength, 1.0, epsilon = 1e-5);
    }

    #[test]
    fn hybrid_with_no_breakdown_falls_back_to_gamut_ranking() {
        // Samples without breakdown — Hybrid should behave like GamutOnly.
        let samples = make_samples(
            &[1.0, 2.0, 3.0],
            &[0.0005, 0.0008, 0.005],
        );
        let result = find_sharpness_direct_with_policy(&samples, 0.001, SelectionPolicy::Hybrid).unwrap();
        assert_eq!(result.selection_mode, SelectionMode::BestSampleWithinBudget);
        // Falls back to max-strength ranking when no composite is available.
        assert_abs_diff_eq!(result.selected_strength, 2.0, epsilon = 1e-5);
    }

    #[test]
    fn composite_only_behaves_like_hybrid() {
        // CompositeOnly currently delegates to Hybrid behavior.
        let samples = vec![
            make_sample_with_composite(1.0, 0.0005, 0.01),
            make_sample_with_composite(2.0, 0.0008, 0.05),
        ];
        let result_composite = find_sharpness_direct_with_policy(&samples, 0.001, SelectionPolicy::CompositeOnly).unwrap();
        let result_hybrid = find_sharpness_direct_with_policy(&samples, 0.001, SelectionPolicy::Hybrid).unwrap();
        assert_eq!(result_composite.selected_strength, result_hybrid.selected_strength);
        assert_eq!(result_composite.selection_mode, result_hybrid.selection_mode);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p r3sizer-core find_sharpness_direct_with_policy 2>&1 | tail -5`
Expected: compilation error — `find_sharpness_direct_with_policy` does not exist.

- [ ] **Step 3: Add `find_sharpness_direct_with_policy` and policy-aware fallback**

Add the `SelectionPolicy` import at the top of `solve.rs`:

```rust
use crate::{CoreError, CrossingStatus, CubicPolynomial, ProbeSample, SelectionMode, SelectionPolicy};
```

Add the new public function after `find_sharpness_direct`:

```rust
/// Direct sample search with policy-aware ranking.
///
/// - `GamutOnly`: ranks by metric_value (existing behavior).
/// - `Hybrid` / `CompositeOnly`: ranks by composite_score from breakdown.
pub fn find_sharpness_direct_with_policy(
    probe_samples: &[ProbeSample],
    target_p0: f32,
    policy: SelectionPolicy,
) -> Result<SolveResult, CoreError> {
    fallback_from_samples(probe_samples, target_p0, policy)
}
```

Also add `find_sharpness_with_policy` (the polynomial path + policy-aware fallback):

```rust
/// Find the optimal sharpening strength with policy-aware fallback.
///
/// Algebraic root finding uses gamut excursion regardless of policy.
/// The policy only affects how fallback candidates are ranked when no
/// polynomial root is available.
pub fn find_sharpness_with_policy(
    poly: &CubicPolynomial,
    target_p0: f64,
    s_min: f64,
    s_max: f64,
    probe_samples: &[ProbeSample],
    policy: SelectionPolicy,
) -> Result<SolveResult, CoreError> {
    // --- Attempt algebraic root finding (policy-independent) ---
    match roots_in_range(poly, target_p0, s_min, s_max) {
        Ok(roots) if !roots.is_empty() => {
            let s_star = roots.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            return Ok(SolveResult {
                selected_strength: s_star as f32,
                selection_mode: SelectionMode::PolynomialRoot,
                crossing_status: CrossingStatus::Found,
            });
        }
        Ok(_) => {}
        Err(_) => {}
    }

    // --- Fallback: policy-aware selection from probe samples ---
    fallback_from_samples(probe_samples, target_p0 as f32, policy)
}
```

Replace the existing `fallback_from_samples` with a policy-aware version:

```rust
fn fallback_from_samples(
    samples: &[ProbeSample],
    p0: f32,
    policy: SelectionPolicy,
) -> Result<SolveResult, CoreError> {
    if samples.is_empty() {
        return Err(CoreError::NoValidRoot {
            reason: "no probe samples available for fallback selection".into(),
        });
    }

    let qualifying: Vec<&ProbeSample> =
        samples.iter().filter(|s| s.metric_value <= p0).collect();

    if let Some(best) = select_best_qualifying(&qualifying, policy) {
        return Ok(SolveResult {
            selected_strength: best.strength,
            selection_mode: SelectionMode::BestSampleWithinBudget,
            crossing_status: CrossingStatus::NotFoundInRange,
        });
    }

    // All samples exceed budget — pick least bad.
    let least_bad = select_least_bad(samples, policy);

    Ok(SolveResult {
        selected_strength: least_bad.strength,
        selection_mode: SelectionMode::LeastBadSample,
        crossing_status: CrossingStatus::NotFoundInRange,
    })
}

/// Among qualifying samples, select the best one per policy.
fn select_best_qualifying<'a>(
    qualifying: &[&'a ProbeSample],
    policy: SelectionPolicy,
) -> Option<&'a ProbeSample> {
    if qualifying.is_empty() {
        return None;
    }
    match policy {
        SelectionPolicy::GamutOnly => {
            // Maximize strength (current behavior).
            qualifying.iter().max_by(|a, b| {
                a.strength.partial_cmp(&b.strength).unwrap()
            }).copied()
        }
        SelectionPolicy::Hybrid | SelectionPolicy::CompositeOnly => {
            // Minimize composite score (best perceptual quality).
            // Fall back to max-strength ranking if no breakdowns available.
            let has_composites = qualifying.iter().any(|s| s.breakdown.is_some());
            if has_composites {
                qualifying.iter().min_by(|a, b| {
                    let ca = composite_score_of(a);
                    let cb = composite_score_of(b);
                    ca.partial_cmp(&cb).unwrap()
                }).copied()
            } else {
                qualifying.iter().max_by(|a, b| {
                    a.strength.partial_cmp(&b.strength).unwrap()
                }).copied()
            }
        }
    }
}

/// Among all samples (budget exceeded), select the least bad per policy.
fn select_least_bad<'a>(
    samples: &'a [ProbeSample],
    policy: SelectionPolicy,
) -> &'a ProbeSample {
    match policy {
        SelectionPolicy::GamutOnly => {
            // Minimize gamut metric_value (current behavior).
            samples.iter()
                .min_by(|a, b| a.metric_value.partial_cmp(&b.metric_value).unwrap())
                .unwrap()
        }
        SelectionPolicy::Hybrid | SelectionPolicy::CompositeOnly => {
            // Minimize composite score.
            let has_composites = samples.iter().any(|s| s.breakdown.is_some());
            if has_composites {
                samples.iter()
                    .min_by(|a, b| {
                        let ca = composite_score_of(a);
                        let cb = composite_score_of(b);
                        ca.partial_cmp(&cb).unwrap()
                    })
                    .unwrap()
            } else {
                samples.iter()
                    .min_by(|a, b| a.metric_value.partial_cmp(&b.metric_value).unwrap())
                    .unwrap()
            }
        }
    }
}

/// Extract composite score from a sample, falling back to metric_value.
#[inline]
fn composite_score_of(sample: &ProbeSample) -> f32 {
    sample.breakdown.as_ref()
        .map(|b| b.composite_score)
        .unwrap_or(sample.metric_value)
}
```

Update the existing `find_sharpness` and `find_sharpness_direct` to delegate to the new functions with `GamutOnly` policy (preserving backward compat):

```rust
pub fn find_sharpness(
    poly: &CubicPolynomial,
    target_p0: f64,
    s_min: f64,
    s_max: f64,
    probe_samples: &[ProbeSample],
) -> Result<SolveResult, CoreError> {
    find_sharpness_with_policy(poly, target_p0, s_min, s_max, probe_samples, SelectionPolicy::GamutOnly)
}

pub fn find_sharpness_direct(
    probe_samples: &[ProbeSample],
    target_p0: f32,
) -> Result<SolveResult, CoreError> {
    find_sharpness_direct_with_policy(probe_samples, target_p0, SelectionPolicy::GamutOnly)
}
```

- [ ] **Step 4: Run the new unit tests**

Run: `cargo test -p r3sizer-core --lib -- solve::tests 2>&1 | tail -20`
Expected: all solve tests pass (both existing and new).

- [ ] **Step 5: Run all existing tests to verify no regression**

Run: `cargo test -p r3sizer-core 2>&1 | tail -10`
Expected: compilation errors in `pipeline.rs` from missing `selection_policy` field — acceptable, fixed in Task 3.

- [ ] **Step 6: Commit**

```bash
git add crates/r3sizer-core/src/solve.rs
git commit -m "feat(solve): add policy-aware fallback selection

find_sharpness_with_policy and find_sharpness_direct_with_policy accept
a SelectionPolicy. GamutOnly preserves existing behavior (max strength
within budget). Hybrid/CompositeOnly rank by composite_score from the
per-probe MetricBreakdown, falling back to gamut ranking when breakdowns
are absent. Existing find_sharpness/find_sharpness_direct delegate to
GamutOnly for backward compatibility.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Wire SelectionPolicy through the pipeline

**Files:**
- Modify: `crates/r3sizer-core/src/pipeline.rs`

- [ ] **Step 1: Import `SelectionPolicy` and update solve call sites**

Add `SelectionPolicy` to the imports at the top of `pipeline.rs`:

```rust
use crate::{
    // ... existing imports ...
    SelectionPolicy,
};
```

Also import the new policy-aware solve functions:

```rust
use crate::solve::{find_sharpness_with_policy, find_sharpness_direct_with_policy};
```

- [ ] **Step 2: Update the fit/solve block to use policy-aware functions**

In the `FitStrategy::DirectSearch` arm (around line 213), change:

```rust
let result = find_sharpness_direct(&probe_samples, params.target_artifact_ratio)?;
```

to:

```rust
let result = find_sharpness_direct_with_policy(
    &probe_samples,
    params.target_artifact_ratio,
    params.selection_policy,
)?;
```

In the `FitStrategy::Cubic` success arm (around line 221), change:

```rust
let result =
    find_sharpness(&poly, p0, s_min, s_max, &probe_samples)?;
```

to:

```rust
let result =
    find_sharpness_with_policy(&poly, p0, s_min, s_max, &probe_samples, params.selection_policy)?;
```

In the `FitStrategy::Cubic` error arm (around line 226), change:

```rust
let result = find_sharpness_direct(
    &probe_samples,
    params.target_artifact_ratio,
)?;
```

to:

```rust
let result = find_sharpness_direct_with_policy(
    &probe_samples,
    params.target_artifact_ratio,
    params.selection_policy,
)?;
```

- [ ] **Step 3: Add `selection_policy` to the diagnostics struct literal**

In the `AutoSharpDiagnostics` construction (around line 456), after the `artifact_metric` field, add:

```rust
        selection_policy: params.selection_policy,
```

- [ ] **Step 4: Verify full build succeeds**

Run: `cargo build --workspace 2>&1 | tail -10`
Expected: clean build with no errors.

- [ ] **Step 5: Run all tests**

Run: `cargo test --workspace 2>&1 | tail -15`
Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/r3sizer-core/src/pipeline.rs
git commit -m "feat(pipeline): wire SelectionPolicy through solve dispatch

Pipeline now passes params.selection_policy to the policy-aware solve
functions. Diagnostics include the selection_policy field. Existing
default (GamutOnly) preserves identical behavior.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Re-export and CLI plumbing

**Files:**
- Modify: `crates/r3sizer-core/src/lib.rs`
- Modify: `crates/r3sizer-cli/src/args.rs`
- Modify: `crates/r3sizer-cli/src/run.rs`

- [ ] **Step 1: Re-export `SelectionPolicy` from lib.rs**

In `lib.rs`, add `SelectionPolicy` to the `pub use types::{...}` block (around line 34):

```rust
pub use types::{
    // ... existing re-exports ...
    SelectionPolicy,
    // ...
};
```

- [ ] **Step 2: Add CLI arg wrapper in args.rs**

After the existing `DiagnosticsLevelArg` enum (around line 158), add:

```rust
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum SelectionPolicyArg {
    GamutOnly,
    Hybrid,
    CompositeOnly,
}

impl From<SelectionPolicyArg> for r3sizer_core::SelectionPolicy {
    fn from(val: SelectionPolicyArg) -> Self {
        match val {
            SelectionPolicyArg::GamutOnly => r3sizer_core::SelectionPolicy::GamutOnly,
            SelectionPolicyArg::Hybrid => r3sizer_core::SelectionPolicy::Hybrid,
            SelectionPolicyArg::CompositeOnly => r3sizer_core::SelectionPolicy::CompositeOnly,
        }
    }
}
```

In the `Cli` struct, after the `diagnostics_level` field (around line 79), add:

```rust
    /// Selection policy: "gamut-only" (default), "hybrid", or "composite-only" (experimental).
    #[arg(long, default_value = "gamut-only")]
    pub selection_policy: SelectionPolicyArg,
```

- [ ] **Step 3: Wire CLI arg into params in run.rs**

In `build_params` (around line 52), add `selection_policy` to the `AutoSharpParams` struct literal:

```rust
        selection_policy: args.selection_policy.into(),
```

- [ ] **Step 4: Verify CLI builds and shows the new flag**

Run: `cargo build -p r3sizer-cli && cargo run -p r3sizer-cli -- --help 2>&1 | grep -A2 selection-policy`
Expected: `--selection-policy` flag appears in help output with the three value options.

- [ ] **Step 5: Commit**

```bash
git add crates/r3sizer-core/src/lib.rs crates/r3sizer-cli/src/args.rs crates/r3sizer-cli/src/run.rs
git commit -m "feat(cli): add --selection-policy flag (gamut-only, hybrid, composite-only)

Re-exports SelectionPolicy from r3sizer-core. CLI defaults to gamut-only
for backward compatibility.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Integration tests

**Files:**
- Modify: `crates/r3sizer-core/tests/integration.rs`

- [ ] **Step 1: Add GamutOnly parity test**

At the bottom of `integration.rs`, add:

```rust
// ---------------------------------------------------------------------------
// SelectionPolicy tests
// ---------------------------------------------------------------------------

use r3sizer_core::SelectionPolicy;

#[test]
fn gamut_only_policy_identical_to_default() {
    let src = gradient_image(64, 64);
    let params_default = default_params(16, 16);
    let params_explicit = AutoSharpParams {
        selection_policy: SelectionPolicy::GamutOnly,
        ..default_params(16, 16)
    };
    let out_default = process_auto_sharp_downscale(&src, &params_default).unwrap();
    let out_explicit = process_auto_sharp_downscale(&src, &params_explicit).unwrap();
    assert_eq!(
        out_default.diagnostics.selected_strength,
        out_explicit.diagnostics.selected_strength,
        "GamutOnly must be identical to default behavior"
    );
    assert_eq!(out_default.image.pixels(), out_explicit.image.pixels());
}
```

- [ ] **Step 2: Add Hybrid safety test**

```rust
#[test]
fn hybrid_policy_respects_gamut_budget() {
    let src = gradient_image(64, 64);
    let params = AutoSharpParams {
        selection_policy: SelectionPolicy::Hybrid,
        diagnostics_level: DiagnosticsLevel::Full,
        ..default_params(16, 16)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let d = &out.diagnostics;

    // If selection was BestSampleWithinBudget, selected probe must be within gamut budget.
    if d.selection_mode == SelectionMode::BestSampleWithinBudget {
        let selected = d.probe_samples.iter()
            .find(|s| (s.strength - d.selected_strength).abs() < 1e-6)
            .expect("selected strength must correspond to a probe sample");
        assert!(
            selected.metric_value <= d.target_artifact_ratio,
            "Hybrid must not select an out-of-budget sample when in-budget exists: metric_value={} > target={}",
            selected.metric_value, d.target_artifact_ratio,
        );
    }
}
```

- [ ] **Step 3: Add Hybrid produces valid result test**

```rust
#[test]
fn hybrid_policy_produces_valid_result() {
    let src = checkerboard(32, 32);
    let params = AutoSharpParams {
        selection_policy: SelectionPolicy::Hybrid,
        ..default_params(8, 8)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.image.width(), 8);
    assert_eq!(out.image.height(), 8);
    for &v in out.image.pixels() {
        assert!(v >= 0.0 && v <= 1.0, "pixel {v} outside [0,1]");
    }
}
```

- [ ] **Step 4: Add diagnostics completeness test**

```rust
#[test]
fn hybrid_diagnostics_include_selection_policy() {
    let src = gradient_image(64, 64);
    let params = AutoSharpParams {
        selection_policy: SelectionPolicy::Hybrid,
        diagnostics_level: DiagnosticsLevel::Full,
        ..default_params(16, 16)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();

    // selection_policy present in diagnostics
    assert_eq!(out.diagnostics.selection_policy, SelectionPolicy::Hybrid);

    // JSON round-trip preserves selection_policy
    let json = serde_json::to_string_pretty(&out.diagnostics).expect("serialize");
    assert!(json.contains("\"selection_policy\""));
    assert!(json.contains("\"hybrid\""));
    let deser: AutoSharpDiagnostics = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(deser.selection_policy, SelectionPolicy::Hybrid);
}

#[test]
fn gamut_only_diagnostics_include_selection_policy() {
    let src = gradient_image(64, 64);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.diagnostics.selection_policy, SelectionPolicy::GamutOnly);

    let json = serde_json::to_string_pretty(&out.diagnostics).expect("serialize");
    assert!(json.contains("\"selection_policy\""));
}
```

- [ ] **Step 5: Add CompositeOnly-as-Hybrid test**

```rust
#[test]
fn composite_only_produces_valid_result() {
    let src = gradient_image(64, 64);
    let params = AutoSharpParams {
        selection_policy: SelectionPolicy::CompositeOnly,
        ..default_params(16, 16)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out.image.width(), 16);
    assert_eq!(out.diagnostics.selection_policy, SelectionPolicy::CompositeOnly);
}
```

- [ ] **Step 6: Run integration tests**

Run: `cargo test -p r3sizer-core --test integration 2>&1 | tail -20`
Expected: all tests pass.

- [ ] **Step 7: Run clippy**

Run: `cargo clippy --workspace -- -D warnings 2>&1 | tail -10`
Expected: no warnings.

- [ ] **Step 8: Commit**

```bash
git add crates/r3sizer-core/tests/integration.rs
git commit -m "test: add integration tests for SelectionPolicy (parity, safety, diagnostics)

Verifies: GamutOnly produces identical output to current default, Hybrid
respects gamut budget, diagnostics include selection_policy in JSON,
CompositeOnly produces valid output.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: Update docs and regenerate TypeScript types

**Files:**
- Modify: `docs/future_work.md`

- [ ] **Step 1: Update future_work.md**

Replace the "Composite metric components (v0.2)" section (lines 56-63) with:

```markdown
### Composite metric components (v0.2) — COMPLETED

All four `MetricComponent` variants are now active and fully implemented:
1. `GamutExcursion` — fraction of channel values outside [0, 1]
2. `HaloRinging` — sign-alternating oscillations near strong edges
3. `EdgeOvershoot` — sharpening exceeding local edge-strength proxy
4. `TextureFlattening` — changes in fine-scale local variance

Configurable weights are supported via `MetricWeights` (default: 1.0, 0.3, 0.3, 0.1).

### Selection policy (v0.2.1)

`SelectionPolicy` controls how fallback candidates are ranked:
- `GamutOnly` (default): gamut excursion drives both fitting and fallback ranking.
- `Hybrid`: gamut excursion is the hard safety constraint; composite score ranks
  fallback candidates. Polynomial fitting still uses gamut excursion.
- `CompositeOnly` (experimental): currently treated as Hybrid. Future work will
  add composite-driven polynomial fitting with a separate `target_selection_score`.

Next steps:
1. Sweep-based comparison of GamutOnly vs Hybrid on a diverse corpus.
2. Add `target_selection_score` parameter for CompositeOnly mode.
3. Investigate composite-driven polynomial fitting (requires monotonicity analysis).
```

- [ ] **Step 2: Regenerate TypeScript types**

Run: `cargo test -p r3sizer-core --features typegen export_typescript_bindings -- --nocapture 2>&1 | tail -5`
Expected: `web/src/types/generated.ts` updated with `SelectionPolicy` type.

- [ ] **Step 3: Run full test suite one final time**

Run: `cargo test --workspace 2>&1 | tail -15`
Expected: all tests pass.

Run: `cargo clippy --workspace -- -D warnings 2>&1 | tail -5`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add docs/future_work.md web/src/types/generated.ts
git commit -m "docs: update future_work.md for selection policy, regenerate TS types

Marks composite metric components as completed. Documents the new
SelectionPolicy axis (GamutOnly, Hybrid, CompositeOnly). Regenerates
TypeScript bindings to include the new enum.

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

## Design Notes

**Why polynomial root is unchanged across policies:**
The polynomial root is already the gamut-constrained optimal point (largest safe strength). Hybrid's value is in the fallback paths where discrete sample ranking matters. A future enhancement could look at the neighborhood around the polynomial root and prefer samples with better composite, but this is not needed for v1.

**Why CompositeOnly is deferred:**
Using composite_score as the polynomial fitting target requires: (a) a separate `target_selection_score` threshold, (b) validation that the composite curve is monotonic enough for stable fitting, (c) sweep-based comparison. The enum variant exists now so the API is stable; the behavior will diverge from Hybrid once these prerequisites are met.

**Backward compatibility:**
- `SelectionPolicy` defaults to `GamutOnly` via `#[default]` and `#[serde(default)]`
- Existing JSON configs, CLI invocations, and WASM calls work without change
- `find_sharpness` / `find_sharpness_direct` (without policy) still exist and delegate to GamutOnly
