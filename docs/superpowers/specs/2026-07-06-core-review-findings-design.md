# Design: `r3sizer-core` review fixes

**Date:** 2026-07-06
**Branch:** `fix/core-review-findings`
**Scope:** Eight findings from a code review of the `r3sizer-core` crate, plus one
small safety swap. All changes are localized to `r3sizer-core` (with a
regenerated TypeScript types file and CLAUDE.md doc corrections).

## Goal

Close correctness/consistency gaps surfaced by review, without changing the
default pixel output except where a fix is the explicit intent (#5). Keep the
optimized probe hot path fast on the default configuration.

---

## #1 — Wire up `SelectionPolicy::Hybrid`/`CompositeOnly` (policy-only gating)

**Problem.** Production probes are always created with `breakdown: None`
(`probe_one_reuse`), so `has_composites` in `solve.rs` is always false and
`Hybrid`/`CompositeOnly` fallback ranking silently degrades to `GamutOnly`. A
user selecting `Hybrid` gets byte-identical results to `GamutOnly`.

**Decision.** Compute the per-probe `MetricBreakdown` **only when
`selection_policy != GamutOnly`** (policy-only gate). This keeps the default
path (`GamutOnly`, incl. the web app's `diagnostics_level: "full"`) on the fast
gamut-only metric, and makes `Hybrid`/`CompositeOnly` genuinely rank by
composite score.

**Changes.**
- Add `AutoSharpParams::needs_probe_breakdown(&self) -> bool` returning
  `self.selection_policy != SelectionPolicy::GamutOnly`.
- `ProbeContext` gains `need_breakdown: bool` and `metric_weights: MetricWeights`.
- `probe_one_reuse`: when `need_breakdown`, attach
  `compute_metric_breakdown(&scratch.rgb, base, base_luma, sharp_luma, artifact_metric, weights)`.
  - Lightness mode: `sharp_luma` is `&scratch.luma` (already the sharpened
    luminance); `base_luma` is `ctx.base_luminance` (fallback: extract from `base`).
  - RGB mode: `sharp_luma` extracted from `scratch.rgb`; `base_luma` as above.
  - `metric_value`/`artifact_ratio` stay computed exactly as today (breakdown is
    additive; it does not change the authoritative selection metric, including
    the `metric_override`/evaluation-color-space path).
- Set the two new `ProbeContext` fields in all three builders: `probe_strengths`,
  `run_two_pass_probing`, `run_probes_from_detail`. The WASM entry points
  (`run_probes_standalone`, `run_probes_from_detail`) already receive `&params`,
  so Hybrid works through the parallel probe pool too (breakdown survives
  serialization via existing `skip_serializing_if`).
- `solve.rs` needs no structural change — its Hybrid ranking already reads
  `composite_score`; it was simply always seeing `None`.

**`rule_switch_to_hybrid` is dormant by design under this gating.** The rule
guards on `policy == GamutOnly`, and under policy-only gating we do not compute
breakdowns for `GamutOnly`. The two never overlap, so the recommendation cannot
fire — the same as today. This is the accepted tradeoff: fast default probe path
over a proactive nudge. Document the rule as dormant-by-design (it requires
per-probe breakdowns under `GamutOnly`, which we intentionally skip for speed).

**CLAUDE.md.** Correct the wording so it states the per-probe `MetricBreakdown`
is populated **under non-`GamutOnly` selection policies**, not unconditionally.

**Tests.**
- Under `Hybrid`, probe samples carry a breakdown and Hybrid fallback selects a
  composite-best sample that differs from `GamutOnly` on a crafted case.
- Under default (`GamutOnly`), probe samples have `breakdown: None` (fast path
  preserved), including with `diagnostics_level: Full`.

---

## #2 — Evaluator cap as a first-class advisory selection stage

**Problem.** `evaluator.rs` and `EvaluatorConfig` docs claim the evaluator is
"purely diagnostic" and "does not alter the pipeline's s\* selection", but
`finish_pipeline` hard-caps `selected_strength` to `prepared.evaluator_cap`, and
the evaluator is on by default. When the cap binds, `selection_mode` still reads
`PolynomialRoot` with no record that capping occurred.

**Decision.** Treat the evaluator as a legitimate stage of strength selection
(`solve → evaluator cap → final`), not a hidden exception. Keep the cap; make it
explainable; fix the docs.

**Changes.**
- `finish_pipeline`: capture `pre_cap = solve_result.selected_strength`. When
  `evaluator_cap` binds (`pre_cap > cap`), record a diagnostic. `selection_mode`
  remains a truthful description of how the **pre-cap** strength was derived.
- New type `EvaluatorCapDiagnostics { cap: f32, strength_before_cap: f32 }`.
- New field on `AutoSharpDiagnostics`:
  `evaluator_cap: Option<EvaluatorCapDiagnostics>` (`Some` only when the cap
  actually lowered s\*; `skip_serializing_if = "Option::is_none"`).
- Reframe docs — `evaluator.rs` module doc, the `QualityEvaluator` trait doc,
  the `EvaluatorConfig` doc, and CLAUDE.md — to describe the evaluator's two
  roles: (a) an **advisory strength cap** that can lower the final s\* (a genuine
  selection stage), and (b) a **post-hoc quality evaluation** (diagnostic). Drop
  the "does not alter s\*" language.

**Tests.**
- When `evaluator_cap < s*`: `diagnostics.evaluator_cap` is `Some` with
  `strength_before_cap == pre-cap` and `cap` == the applied cap; `selected_strength`
  equals the cap; `selection_mode` unchanged.
- When the cap does not bind: `diagnostics.evaluator_cap` is `None`.

---

## #3 — `full_total_us` includes the probing phase

**Problem.** `total_us` starts at `finish_pipeline` entry (after probing).
`full_total_us` adds back the `PreparedBase` stages but never adds `probing_us`,
so the reported total is smaller than the sum of its own rows.

**Change.** Add `+ probing_us` to the `full_total_us` sum. Correct for both the
internal path and `process_from_prepared_with_probes` (in both, `total_us`
starts after probing).

**Test.** Assert `timing.total_us >= timing.probing_us` for a real run.

---

## #4 — Defensive sort for externally-supplied samples

**Problem.** `s_max` uses `.last()`, `check_monotonicity` scans adjacent pairs,
and `find_dense_window` scans `windows(2)` — all incorrect if externally
collected samples arrive unsorted. The current JS pool sorts, so there is no live
bug, but the invariant lives only in the caller.

**Changes.**
- Sort `probe_samples` by ascending strength at the top of `finish_pipeline`
  (covers `process_from_prepared` and `process_from_prepared_with_probes`);
  idempotent for already-sorted input.
- Sort a local copy of `coarse_samples` at the top of `resolve_dense_strengths`
  before `find_dense_window`.
- Document both entry points: "samples may arrive in any order; sorted
  internally by ascending strength."

**Test.** `finish_pipeline` (via a public entry) and `resolve_dense_strengths`
produce correct results when given deliberately unsorted samples.

---

## #5 — `ClampPolicy::Normalize` no longer brightens in-gamut images

**Problem.** `v / max_val` with `max_val = 0.9` scales everything up ~11%, so a
purely in-gamut image is brightened by a policy meant to handle out-of-range
values.

**Change.** `let denom = max_val.max(1.0);` then `*v = (*v / denom).max(0.0)`.
Images with `max < 1.0` pass through unchanged; only values `> 1.0` are
compressed; negatives remain floored to 0. Simplify away the now-redundant
`max_val > 0.0` branch. Update the `ClampPolicy::Normalize` doc to describe the
`max(·, 1.0)` behavior.

**Test.** Normalize leaves a `max < 1.0` image unchanged; compresses a
`max > 1.0` image; floors negatives.

---

## #6 — `ProbeConfig::Explicit` validation matches its documentation

**Problem.** The doc says "must have >= 4 distinct, positive values" but
`resolve()` only checks length. Non-positive or all-identical strengths pass
`validate()`.

**Change.** In `resolve()`'s `Explicit` branch: reject any value `<= 0`, and
reject fewer than 4 **distinct** values (checked on the sorted list within an
epsilon). Flows through `validate()` automatically (its `_ =>` arm calls
`resolve()`).

**Test.** `resolve()` rejects a list with a non-positive value and a list with
fewer than 4 distinct values; accepts a valid list.

---

## #7 — Fix stale `pipeline_mode` doc

**Problem.** The `pipeline_mode` field doc claims `PipelineMode::apply` "is
called automatically during `AutoSharpParams::validate`" — it is not (`validate`
takes `&self`).

**Change.** Correct the doc to state that callers must invoke `.resolved()`
before pipeline entry (as CLI and WASM do). Doc-only.

---

## #8 — Adaptive resize deduplicates aliased kernels

**Problem.** `MitchellNetravali` and `CatmullRom` both map to
`FilterType::CatmullRom`, but are distinct `ResizeKernel` values. The default
`KernelTable` contains both, so `downscale_adaptive` runs the same resize twice,
and diagnostics report "MitchellNetravali" for pixels actually produced by
CatmullRom.

**Change.** Canonicalize aliased kernels (`MitchellNetravali → CatmullRom`) so
`downscale_adaptive` runs one resize per distinct operation, per-pixel selection
indexes the canonical result, and `kernels_used`/`per_kernel_pixel_count` report
the actual kernel used (CatmullRom). Lanczos3 stays separate (it uses the
staged `downscale` path, not the `image`-crate filter).

**Test.** `downscale_adaptive` with a table containing both `MitchellNetravali`
and `CatmullRom` runs a single CatmullRom resize; `kernels_used` contains no
duplicate operation; per-kernel counts sum to the output pixel count.

---

## Extra — `solve.rs` fallback ranking uses `total_cmp`

`select_best_qualifying` and `select_least_bad` use `partial_cmp(...).unwrap()`,
which panics on NaN and contradicts the "never panics" contract. #1 makes the
composite-ranking path genuinely reachable. Swap the comparisons in those two
functions to `f32::total_cmp`. Two-line change; no behavior change for finite
inputs.

---

## Cross-cutting

- Regenerate the ts-rs TypeScript bindings (new `EvaluatorCapDiagnostics` type +
  `evaluator_cap` field) via
  `cargo test -p r3sizer-core --features typegen export_typescript_bindings`
  and commit the generated file (path per CLAUDE.md).
- All new/changed behavior covered by unit tests as listed per item.
- `cargo test -p r3sizer-core` and `cargo clippy --workspace -- -D warnings`
  must be green.

## Explicitly out of scope (review "minor notes")

`fit_cubic`/`fit_cubic_with_quality` dedup, kernel/closure rebuilds in
`finish_pipeline`, the double luminance extraction in `prepare_base`, and the
stale "Defer sqrt" comment in `chroma_guard.rs`. (The `total_cmp` note is
in scope above because #1 exercises that path.)

## Files touched

- `crates/r3sizer-core/src/pipeline.rs` — #1, #2, #3, #4
- `crates/r3sizer-core/src/types.rs` — #1 helper, #2 type/field, #5 doc, #6
  validation, #7 doc
- `crates/r3sizer-core/src/solve.rs` — `total_cmp`
- `crates/r3sizer-core/src/resize_strategy.rs` — #8
- `crates/r3sizer-core/src/evaluator.rs` — #2 docs
- `crates/r3sizer-core/src/recommendations.rs` — #1 doc (dormancy note)
- `CLAUDE.md` — #1 and #2 wording
- generated TypeScript bindings file — regenerated
