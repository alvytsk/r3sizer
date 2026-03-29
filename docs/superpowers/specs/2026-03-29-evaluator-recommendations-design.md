# Evaluator Recommendations — Design Spec

**Date:** 2026-03-29
**Branch:** refactor/optimizing
**Scope:** Deterministic recommendation system that translates pipeline diagnostics into actionable `AutoSharpParams` patches. The polynomial solver remains authoritative; the evaluator acts as advisor.

---

## 1. Summary

The heuristic quality evaluator currently produces a `suggested_strength` that is displayed as an advisory message but cannot be acted upon from the web UI. This spec replaces that advisory-only output with a structured recommendation system.

Each recommendation is a deterministic, rule-based mapping from pipeline diagnostics and image features to a concrete `ParamPatch` — a self-contained partial update to `AutoSharpParams`. "Apply" in the UI means: merge the patch into params, rerun the solver. The solver still selects final `s*`, but under better settings.

**Design contract: Evaluator = advisor, pipeline = decision-maker.**

---

## 2. Architecture

### 2.1 New module: `recommendations.rs`

A new module in `r3sizer-core`, separate from `evaluator.rs`. The evaluator is about predicting quality and suggesting strength; the recommendation generator is about translating diagnostics into param patches. These are different concerns.

```
evaluator.rs   → QualityEvaluator trait, HeuristicEvaluator, feature extraction
recommendations.rs → generate_recommendations(), rule engine, patch construction
```

### 2.2 Entry point

```rust
pub fn generate_recommendations(
    diagnostics: &AutoSharpDiagnostics,
    params: &AutoSharpParams,
) -> Vec<Recommendation>
```

Called unconditionally in `pipeline.rs` after diagnostics are fully assembled, before returning `ProcessOutput`. Does **not** depend on `evaluator_result` being present — most rules fire from solver/diagnostics state alone. `evaluator_result` is one optional signal consumed by rules that need image features.

### 2.3 Pipeline integration

In `pipeline.rs`, after the diagnostics struct is complete:

```rust
diagnostics.recommendations =
    crate::recommendations::generate_recommendations(&diagnostics, params);
```

No guard on `evaluator_config`. The recommendation pass is always active. Disabling the evaluator via the UI toggle removes `evaluator_result` from diagnostics, which causes evaluator-dependent rules (3, 6) to skip, but diagnostics-only rules (1, 2, 4, 5) still fire.

---

## 3. New types

All types in `types.rs` with the standard derives and `ts-rs` gating.

### 3.1 `RecommendationKind`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[ts(export)]
#[serde(snake_case)]
pub enum RecommendationKind {
    SwitchToContentAdaptive,
    LowerStrongEdgeGain,
    RaiseArtifactBudget,
    SwitchToLightness,
    WidenProbeRange,
    LowerSigma,
}
```

Used for UI labeling and advice deduplication. The real action is in `patch`.

### 3.2 `Severity`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[ts(export)]
#[serde(snake_case)]
pub enum Severity {
    Info,
    Suggestion,
    Warning,
}
```

Display-only. Does not affect which patch gets applied or how.

### 3.3 `ParamPatch`

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[ts(export)]
pub struct ParamPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sharpen_strategy: Option<SharpenStrategy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_artifact_ratio: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sharpen_mode: Option<SharpenMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe_strengths: Option<ProbeConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sharpen_sigma: Option<f32>,
}
```

**Invariant:** Every patch is fully self-contained and directly applicable. For nested types like `SharpenStrategy::ContentAdaptive`, the patch carries the full replacement value — no deep-merge logic in the UI or store.

### 3.4 `Recommendation`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
#[ts(export)]
pub struct Recommendation {
    pub kind: RecommendationKind,
    pub severity: Severity,
    /// Confidence in [0, 1]. Display-only — does not affect patch content.
    pub confidence: f32,
    /// Human-readable explanation of why this recommendation was generated.
    pub reason: String,
    /// Self-contained param patch. Apply via `updateParams(patch)`.
    pub patch: ParamPatch,
}
```

### 3.5 Diagnostics field

Added to `AutoSharpDiagnostics`:

```rust
/// Actionable recommendations derived from pipeline diagnostics.
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub recommendations: Vec<Recommendation>,
```

---

## 4. Recommendation rules

All rules are evaluated in order. All matching rules fire (no early exit). A conflict-resolution pass runs after all rules before returning.

### Rule 1 — `SwitchToContentAdaptive`

| | |
|---|---|
| **Trigger** | `params.sharpen_strategy` is `Uniform` AND `evaluator_result.features.edge_density > 0.15` |
| **Patch** | `sharpen_strategy`: full `ContentAdaptive` with default classification and backoff, `gain_table.strong_edge` set to 0.5 (other gains at default) |
| **Severity** | Suggestion |
| **Confidence** | `(edge_density - 0.15).min(0.25) / 0.25` → [0, 1] |
| **Reason** | "Image has high edge density ({x:.0}%). Content-adaptive sharpening reduces halos by varying strength per region." |
| **Requires** | `evaluator_result` (skips if evaluator disabled) |

### Rule 2 — `LowerStrongEdgeGain`

| | |
|---|---|
| **Trigger** | `params.sharpen_strategy` is `ContentAdaptive` AND `diagnostics.region_coverage.risky_halo_zone_fraction > 0.15` AND current `gain_table.strong_edge > 0.5` |
| **Patch** | `sharpen_strategy`: clone of current `ContentAdaptive` with `gain_table.strong_edge` halved |
| **Severity** | Suggestion if fraction in (0.15, 0.25], Warning if > 0.25 |
| **Confidence** | `(fraction - 0.15).min(0.25) / 0.25` |
| **Reason** | "{x:.0}% of the image is in the halo risk zone. Reducing strong-edge gain will temper sharpening near prominent edges." |
| **Requires** | `region_coverage` (always present when `ContentAdaptive`) |

### Rule 3 — `RaiseArtifactBudget`

Translates the evaluator's suggested strength into a budget via the fitted curve.

| | |
|---|---|
| **Trigger** | `evaluator_result.suggested_strength` exists AND `abs(s_eval - s_cur) / s_cur > 0.30` AND `fit_coefficients` available AND `s_eval` is within probe range |
| **Computation** | Evaluate `P_hat(s_eval) = a·s³ + b·s² + c·s + d` using fit coefficients. If no fit, linear-interpolate from two nearest probe samples. |
| **Patch** | `target_artifact_ratio: P_hat(s_eval)` (clamped to [1e-5, 0.05]) |
| **Severity** | Info |
| **Confidence** | `evaluator_result.confidence` (pass-through) |
| **Reason** | "Adjusting artifact budget to {new_p0:.1e} (from {old_p0:.1e}) lets the solver target strength {s_eval:.3} naturally." |
| **Requires** | `evaluator_result` (skips if evaluator disabled). Skips if `s_eval` is outside probe range (WidenProbeRange handles that). |

### Rule 4 — `SwitchToLightness`

| | |
|---|---|
| **Trigger** | `params.sharpen_mode == Rgb` AND `evaluator_result.features.edge_density > 0.10` |
| **Patch** | `sharpen_mode: Lightness` |
| **Severity** | Suggestion |
| **Confidence** | `(edge_density - 0.10).min(0.20) / 0.20` |
| **Reason** | "RGB sharpening on edge-rich content risks color fringing. Lightness mode sharpens luminance only." |
| **Requires** | `evaluator_result` (skips if evaluator disabled) |

Note: Rules 1 and 4 require evaluator features. If a future version extracts features independently of the evaluator, these rules should switch to that source.

### Rule 5 — `WidenProbeRange`

| | |
|---|---|
| **Trigger A** | `fit_quality.r_squared < 0.85` |
| **Trigger B** | `selected_strength` is within 10% of the min or max probe value |
| **Patch** | `probe_strengths: Explicit([...])` — always emits `Explicit` form regardless of user's current `ProbeConfig` style. Extends current range by ~50% in both directions with 2 additional intermediate points. |
| **Severity** | Warning (trigger A), Suggestion (trigger B only) |
| **Confidence** | Trigger A: `(0.85 - r_squared).min(0.3) / 0.3`. Trigger B: 0.6 fixed |
| **Reason** | A: "Curve fit quality is low (R²={r2:.2}). Wider probe coverage should improve accuracy." B: "Selected strength is near the probe boundary. Widening the range gives the solver more room." |
| **Requires** | `probe_samples` (always present) |

Probe generation logic: sort existing probes, compute min/max, extend by 50% in each direction (floor at 0.01, cap at 3.0), add 2 evenly-spaced points in the extensions, sort and deduplicate.

### Rule 6 — `LowerSigma`

| | |
|---|---|
| **Trigger** | `evaluator_result.features.edge_density > 0.25` AND `params.sharpen_sigma > 1.5` |
| **Patch** | `sharpen_sigma: (current * 0.6)` rounded to nearest 0.1 |
| **Severity** | Info |
| **Confidence** | `(edge_density - 0.25).min(0.15) / 0.15` |
| **Reason** | "This image is detail-rich (edge density {x:.0}%). A lower sigma sharpens finer features with less halo risk." |
| **Requires** | `evaluator_result` (skips if evaluator disabled) |

---

## 5. Conflict resolution

Conflicts are resolved in `generate_recommendations()` before returning — the UI receives a clean, non-conflicting list.

### 5.1 Rules

1. **`SwitchToContentAdaptive` and `LowerStrongEdgeGain` are mutually exclusive by construction.** Rule 1 requires `Uniform` strategy; Rule 2 requires `ContentAdaptive`. At most one fires per run. Both touch `sharpen_strategy`, so mutual exclusivity is load-bearing for invariant (2).

2. **No two recommendations touch the same `ParamPatch` field.** This holds by construction:
   - `SwitchToContentAdaptive` xor `LowerStrongEdgeGain` → `sharpen_strategy`
   - `RaiseArtifactBudget` → `target_artifact_ratio`
   - `SwitchToLightness` → `sharpen_mode`
   - `WidenProbeRange` → `probe_strengths`
   - `LowerSigma` → `sharpen_sigma`

3. **Apply-all is safe** because of invariant (2). The UI applies patches sequentially; order doesn't matter since fields don't overlap.

4. **If a future rule breaks invariant (2)**, the generator must detect the conflict and either collapse the recommendations or drop the lower-priority one before returning. The invariant is enforced by the generator, never by the UI.

### 5.2 Guarantee

```
∀ r1, r2 ∈ recommendations where r1 ≠ r2:
    fields_set(r1.patch) ∩ fields_set(r2.patch) = ∅
```

This is enforced by the generator, not by the UI.

---

## 6. Web UI changes

### 6.1 Advice deduplication (`DiagnosticsPanel.tsx`)

`generateAdvice()` receives the recommendation kinds present in `diagnostics.recommendations` and suppresses overlapping generic advice:

| Recommendation present | Suppress advice card |
|---|---|
| `SwitchToContentAdaptive` | "High halo-risk content" |
| `LowerStrongEdgeGain` | "High halo-risk content" |
| `WidenProbeRange` | "Fit quality poor" |
| Any recommendation | "Quality evaluator suggests different strength" (removed entirely) |

The old evaluator advice card (lines 671-682) is replaced by the recommendation cards.

### 6.2 Recommendation cards

Rendered in the Advice tab, after existing non-suppressed advice cards. Each card:

```
┌─────────────────────────────────────────────────┐
│ ☆  Content-adaptive sharpening recommended      │  ← kind → display label
│                                                 │
│  Image has high edge density (18%). Content-    │  ← reason
│  adaptive sharpening reduces halos by varying   │
│  strength per region.                           │
│                                         [Apply] │  ← updateParams(patch)
└─────────────────────────────────────────────────┘
```

When > 1 recommendation, an "Apply all recommendations" link appears below all cards.

### 6.3 Severity → visual style

| Severity | Border | Background | Icon/title color |
|---|---|---|---|
| Warning | `border-primary/25` | `bg-primary/5` | `text-primary` (orange) |
| Suggestion | `border-chart-2/25` | `bg-chart-2/5` | `text-chart-2` (existing tip style) |
| Info | `border-muted-foreground/15` | `bg-muted/5` | `text-muted-foreground` (neutral) |

Info is neutral/muted — not green/success. Success remains reserved for "Looking good — no issues detected."

### 6.4 Kind → display label

| Kind | Label |
|---|---|
| `SwitchToContentAdaptive` | "Content-adaptive sharpening recommended" |
| `LowerStrongEdgeGain` | "Reduce strong-edge gain" |
| `RaiseArtifactBudget` | "Raise artifact budget" |
| `SwitchToLightness` | "Switch to lightness mode" |
| `WidenProbeRange` | "Widen probe range" |
| `LowerSigma` | "Lower blur sigma" |

### 6.5 Apply flow

1. User clicks "Apply" on a recommendation card
2. UI calls `updateParams(recommendation.patch)` — standard store spread, no special merge
3. Recommendation cards clear (diagnostics are stale)
4. User clicks Process (or auto-reprocess triggers)
5. New run may produce different/fewer/no recommendations

"Apply all" iterates recommendations in order, calling `updateParams` for each. Field non-overlap guarantees order-independence.

### 6.6 Evaluator toggle (`ParameterPanel.tsx`)

Remains as-is in the Advanced section. Disabling the evaluator removes `evaluator_result` from diagnostics, which causes rules 1, 3, 4, 6 to skip (they require evaluator features). Rules 2, 5 still fire from diagnostics-only signals. The toggle tooltip should state: "Enables the heuristic quality evaluator. When off, recommendations based on image features are unavailable."

---

## 7. File changes

| File | Change |
|---|---|
| `crates/r3sizer-core/src/types.rs` | Add `RecommendationKind`, `Severity`, `ParamPatch`, `Recommendation`. Add `recommendations: Vec<Recommendation>` to `AutoSharpDiagnostics`. |
| `crates/r3sizer-core/src/recommendations.rs` | New module. `generate_recommendations()` with 6 rules + conflict resolution. |
| `crates/r3sizer-core/src/lib.rs` | Add `pub mod recommendations;` |
| `crates/r3sizer-core/src/pipeline.rs` | One call: `diagnostics.recommendations = recommendations::generate_recommendations(&diagnostics, params);` after diagnostics assembly. |
| `web/src/types/generated.ts` | Regenerated via `cargo test --features typegen`. |
| `web/src/types/wasm-types.ts` | Re-export `Recommendation`, `RecommendationKind`, `Severity`, `ParamPatch`. |
| `web/src/components/DiagnosticsPanel.tsx` | Replace evaluator advice card with recommendation cards. Add "Apply" buttons. Add severity styles for Info. Suppress duplicate advice. |
| `web/src/components/ParameterPanel.tsx` | Update evaluator toggle tooltip. |

No changes to `processor-store.ts` — `updateParams(partial)` already handles arbitrary `Partial<AutoSharpParams>`.

---

## 8. Testing

### 8.1 Rust unit tests (`recommendations.rs`)

Each rule gets at least two tests:
- **Fires when expected**: construct minimal `AutoSharpDiagnostics` + `AutoSharpParams` that trigger the rule, assert recommendation with expected kind and non-empty patch.
- **Skips when not triggered**: same but with values below thresholds, assert empty or different recommendations.

Conflict resolution tests:
- Both `SwitchToContentAdaptive` and `LowerStrongEdgeGain` triggered → single recommendation returned.
- No two recommendations share a `ParamPatch` field.

### 8.2 Patch application test

Round-trip test: generate a recommendation, serialize its patch to JSON, deserialize as `Partial<AutoSharpParams>` equivalent, merge into default params, verify the expected field changed.

### 8.3 Integration test

Full pipeline run on a test image → assert `diagnostics.recommendations` is non-empty for an image known to trigger at least one rule (e.g., high-edge-density image in RGB mode → `SwitchToLightness`).
