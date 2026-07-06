# r3sizer-core Review Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close eight correctness/consistency findings (plus one NaN-safety swap) from the `r3sizer-core` code review, keeping the default probe hot path fast.

**Architecture:** All changes are localized to the `r3sizer-core` crate: parameter validation, the pipeline orchestrator, the solver's fallback comparators, the adaptive-resize dispatcher, and the ts-rs typegen output. A new serialized diagnostics type (`EvaluatorCapDiagnostics`) is added and the TypeScript bindings are regenerated.

**Tech Stack:** Rust (workspace crate `r3sizer-core`), `cargo test`, `cargo clippy`, ts-rs for TypeScript generation.

## Global Constraints

- Design spec: `docs/superpowers/specs/2026-07-06-core-review-findings-design.md`.
- All numeric pixel work is f32; polynomial fitting is f64 (do not change).
- `sharpen.rs` never clamps; clamping happens only in `apply_clamp_policy`.
- `cargo test -p r3sizer-core` and `cargo clippy --workspace -- -D warnings` must be green at the end of every task.
- Serialized types use `#[cfg_attr(feature = "typegen", derive(TS))]`; new serializable types must be added to `crates/r3sizer-core/tests/typegen.rs` and re-exported from `crates/r3sizer-core/src/lib.rs`.
- Work happens on branch `fix/core-review-findings` (already created off `main`).
- Commit after each task with a descriptive message.

---

### Task 1: #7 — Correct the `pipeline_mode` doc comment

**Files:**
- Modify: `crates/r3sizer-core/src/types.rs:549-554`

**Interfaces:**
- Consumes: nothing.
- Produces: nothing (doc-only).

- [ ] **Step 1: Replace the stale doc comment**

In `crates/r3sizer-core/src/types.rs`, find:

```rust
    // --- Runtime mode ---
    /// Performance-quality tradeoff.  When set, [`PipelineMode::apply`] is
    /// called automatically during [`AutoSharpParams::validate`], overriding
    /// the speed-sensitive fields before pipeline execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline_mode: Option<PipelineMode>,
```

Replace with:

```rust
    // --- Runtime mode ---
    /// Performance-quality tradeoff.  Not applied automatically: call
    /// [`AutoSharpParams::resolved`] before pipeline entry to fold this mode's
    /// overrides into the speed-sensitive fields (as the CLI and WASM callers
    /// do).  [`AutoSharpParams::validate`] does not modify params.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline_mode: Option<PipelineMode>,
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p r3sizer-core`
Expected: builds with no errors.

- [ ] **Step 3: Commit**

```bash
git add crates/r3sizer-core/src/types.rs
git commit -m "docs(core): correct pipeline_mode auto-apply claim (#7)"
```

---

### Task 2: #6 — Enforce positive, distinct `ProbeConfig::Explicit` values

**Files:**
- Modify: `crates/r3sizer-core/src/types.rs:277-317` (the `ProbeConfig::resolve` method)
- Test: `crates/r3sizer-core/src/types.rs` (existing `#[cfg(test)] mod adaptive_tests` or a new test near `ProbeConfig`)

**Interfaces:**
- Consumes: `ProbeConfig::resolve(&self) -> Result<Vec<f32>, CoreError>`.
- Produces: `resolve()` now rejects non-positive values and fewer than 4 distinct values.

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod adaptive_tests` block in `crates/r3sizer-core/src/types.rs`:

```rust
    #[test]
    fn explicit_rejects_non_positive_values() {
        let cfg = ProbeConfig::Explicit(vec![0.5, 0.0, 1.0, 1.5]);
        assert!(matches!(cfg.resolve(), Err(CoreError::InvalidParams(_))));
        let cfg = ProbeConfig::Explicit(vec![0.5, -0.2, 1.0, 1.5]);
        assert!(matches!(cfg.resolve(), Err(CoreError::InvalidParams(_))));
    }

    #[test]
    fn explicit_rejects_fewer_than_four_distinct() {
        // Four values but only two distinct.
        let cfg = ProbeConfig::Explicit(vec![0.5, 0.5, 1.0, 1.0]);
        assert!(matches!(cfg.resolve(), Err(CoreError::InvalidParams(_))));
    }

    #[test]
    fn explicit_accepts_four_distinct_positive() {
        let cfg = ProbeConfig::Explicit(vec![0.25, 0.5, 1.0, 2.0]);
        let out = cfg.resolve().unwrap();
        assert_eq!(out, vec![0.25, 0.5, 1.0, 2.0]);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p r3sizer-core explicit_ -- --nocapture`
Expected: `explicit_rejects_non_positive_values` and `explicit_rejects_fewer_than_four_distinct` FAIL (current `resolve()` only checks length).

- [ ] **Step 3: Add the validation**

In `ProbeConfig::resolve`, find the `Explicit` arm:

```rust
            ProbeConfig::Explicit(v) => {
                if v.len() < 4 {
                    return Err(CoreError::InvalidParams(
                        "explicit probe list must have at least 4 values".into(),
                    ));
                }
                v.clone()
            }
```

Replace with:

```rust
            ProbeConfig::Explicit(v) => {
                if v.len() < 4 {
                    return Err(CoreError::InvalidParams(
                        "explicit probe list must have at least 4 values".into(),
                    ));
                }
                if v.iter().any(|&s| s <= 0.0) {
                    return Err(CoreError::InvalidParams(
                        "explicit probe values must all be positive".into(),
                    ));
                }
                v.clone()
            }
```

Then find the tail of `resolve` (after the `match`):

```rust
        values.sort_by(|a, b| a.partial_cmp(b).unwrap());
        Ok(values)
    }
```

Replace with:

```rust
        values.sort_by(|a, b| a.partial_cmp(b).unwrap());
        // Reject fewer than 4 distinct values (e.g. an Explicit list of
        // duplicates), which would otherwise produce a degenerate fit.
        let distinct = values.windows(2).filter(|w| (w[1] - w[0]).abs() > 1e-9).count() + 1;
        if distinct < 4 {
            return Err(CoreError::InvalidParams(
                "probe strengths must include at least 4 distinct values".into(),
            ));
        }
        Ok(values)
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p r3sizer-core explicit_ -- --nocapture`
Expected: all three PASS.

- [ ] **Step 5: Run the full crate tests + clippy**

Run: `cargo test -p r3sizer-core && cargo clippy -p r3sizer-core -- -D warnings`
Expected: all green (confirms existing `Range` configs still resolve — linear spacing is already distinct).

- [ ] **Step 6: Commit**

```bash
git add crates/r3sizer-core/src/types.rs
git commit -m "fix(core): validate positive, distinct Explicit probe values (#6)"
```

---

### Task 3: #5 — `ClampPolicy::Normalize` must not brighten in-gamut images

**Files:**
- Modify: `crates/r3sizer-core/src/pipeline.rs:643-667` (`apply_clamp_policy`)
- Modify: `crates/r3sizer-core/src/types.rs:388-390` (`ClampPolicy::Normalize` doc)
- Test: new `#[cfg(test)] mod tests` at the end of `crates/r3sizer-core/src/pipeline.rs`

**Interfaces:**
- Consumes: `fn apply_clamp_policy(image: &mut LinearRgbImage, policy: ClampPolicy)` (private; tests live in the same module).
- Produces: Normalize divides by `max(global_max, 1.0)`.

- [ ] **Step 1: Write the failing test**

Add a new module at the very end of `crates/r3sizer-core/src/pipeline.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ClampPolicy;

    #[test]
    fn normalize_leaves_in_gamut_image_unchanged() {
        // Max is 0.9 (< 1.0): the image is already in gamut, so it must pass
        // through unchanged rather than being brightened.
        let mut img = LinearRgbImage::new(1, 2, vec![0.1, 0.5, 0.9, 0.2, 0.4, 0.6]).unwrap();
        let before = img.pixels().to_vec();
        apply_clamp_policy(&mut img, ClampPolicy::Normalize);
        for (a, b) in before.iter().zip(img.pixels()) {
            assert!((a - b).abs() < 1e-6, "in-gamut value changed: {a} -> {b}");
        }
    }

    #[test]
    fn normalize_compresses_out_of_range_and_floors_negatives() {
        // Max is 2.0 (> 1.0): everything scales by 1/2.0; the negative floors to 0.
        let mut img = LinearRgbImage::new(1, 2, vec![2.0, 1.0, 0.0, -0.5, 0.5, 0.5]).unwrap();
        apply_clamp_policy(&mut img, ClampPolicy::Normalize);
        let p = img.pixels();
        assert!((p[0] - 1.0).abs() < 1e-6); // 2.0 / 2.0
        assert!((p[1] - 0.5).abs() < 1e-6); // 1.0 / 2.0
        assert!((p[3] - 0.0).abs() < 1e-6); // -0.5 floored to 0
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p r3sizer-core normalize_ -- --nocapture`
Expected: `normalize_leaves_in_gamut_image_unchanged` FAILS (current code divides by 0.9, brightening).

- [ ] **Step 3: Fix `apply_clamp_policy`**

In `crates/r3sizer-core/src/pipeline.rs`, find the `Normalize` arm:

```rust
        ClampPolicy::Normalize => {
            let max_val = image
                .pixels()
                .iter()
                .copied()
                .fold(f32::NEG_INFINITY, f32::max);
            if max_val > 0.0 {
                for v in image.pixels_mut() {
                    *v = (*v / max_val).max(0.0);
                }
            } else {
                for v in image.pixels_mut() {
                    *v = 0.0;
                }
            }
        }
```

Replace with:

```rust
        ClampPolicy::Normalize => {
            let max_val = image
                .pixels()
                .iter()
                .copied()
                .fold(f32::NEG_INFINITY, f32::max);
            // Divide by max(global maximum, 1.0): in-gamut images (max <= 1.0)
            // pass through unchanged; only values above 1.0 are compressed.
            // Negatives are floored to 0.
            let denom = max_val.max(1.0);
            for v in image.pixels_mut() {
                *v = (*v / denom).max(0.0);
            }
        }
```

- [ ] **Step 4: Update the enum doc**

In `crates/r3sizer-core/src/types.rs`, find:

```rust
    /// Rescale entire image by its global maximum.
    Normalize,
```

Replace with:

```rust
    /// Rescale by `max(global maximum, 1.0)`: images already in `[0, 1]` pass
    /// through unchanged; only images with values above 1.0 are compressed.
    /// Negative values are floored to 0.
    Normalize,
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p r3sizer-core normalize_ -- --nocapture`
Expected: both PASS.

- [ ] **Step 6: Run crate tests + clippy**

Run: `cargo test -p r3sizer-core && cargo clippy -p r3sizer-core -- -D warnings`
Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add crates/r3sizer-core/src/pipeline.rs crates/r3sizer-core/src/types.rs
git commit -m "fix(core): Normalize clamp no longer brightens in-gamut images (#5)"
```

---

### Task 4: #8 — Deduplicate aliased kernels in adaptive resize

**Files:**
- Modify: `crates/r3sizer-core/src/resize_strategy.rs:29-140` (`downscale_with_kernel` neighbourhood; add `canonical_kernel`, update `downscale_adaptive`)
- Test: `crates/r3sizer-core/src/resize_strategy.rs` (existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `to_filter_type`, `downscale_with_kernel`, `KernelTable::kernel_for`.
- Produces: `fn canonical_kernel(kernel: ResizeKernel) -> ResizeKernel`; `downscale_adaptive` runs one resize per distinct operation.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `crates/r3sizer-core/src/resize_strategy.rs`:

```rust
    #[test]
    fn adaptive_dedups_mitchell_into_catmullrom() {
        // Mitchell and CatmullRom alias to the same filter; the table below has
        // both, but only CatmullRom (their canonical form) + Lanczos3 should be
        // resized/reported — never MitchellNetravali.
        let src = gradient_image(32, 32);
        let target = ImageSize { width: 8, height: 8 };
        let table = KernelTable {
            flat: ResizeKernel::MitchellNetravali,
            textured: ResizeKernel::CatmullRom,
            strong_edge: ResizeKernel::Lanczos3,
            microtexture: ResizeKernel::CatmullRom,
            risky_halo_zone: ResizeKernel::MitchellNetravali,
        };
        let (result, diag) =
            downscale_adaptive(&src, target, &ClassificationParams::default(), &table).unwrap();
        assert_eq!(result.width(), 8);
        assert!(!diag.kernels_used.contains(&ResizeKernel::MitchellNetravali));
        assert!(diag.kernels_used.contains(&ResizeKernel::CatmullRom));
        // No key named "MitchellNetravali" in the per-kernel counts.
        assert!(!diag.per_kernel_pixel_count.contains_key("MitchellNetravali"));
        let total: u32 = diag.per_kernel_pixel_count.values().sum();
        assert_eq!(total, 64);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p r3sizer-core adaptive_dedups_mitchell -- --nocapture`
Expected: FAIL — `kernels_used` currently contains `MitchellNetravali`.

- [ ] **Step 3: Add `canonical_kernel`**

In `crates/r3sizer-core/src/resize_strategy.rs`, immediately after the `to_filter_type` function, add:

```rust
/// Collapse kernels that map to the same underlying resize operation.
///
/// `MitchellNetravali` currently aliases to the same `image`-crate filter as
/// `CatmullRom` (see [`to_filter_type`]), so treat them as one operation: this
/// avoids running the identical resize twice and reports the kernel actually
/// used.  Lanczos3 stays distinct (it uses the staged `downscale` path).
fn canonical_kernel(kernel: ResizeKernel) -> ResizeKernel {
    match kernel {
        ResizeKernel::MitchellNetravali => ResizeKernel::CatmullRom,
        other => other,
    }
}
```

- [ ] **Step 4: Canonicalize in `downscale_adaptive`**

In `downscale_adaptive`, find the kernel-set construction:

```rust
    // 2. Determine which distinct kernels are needed
    let all_kernels: Vec<ResizeKernel> = [
        kernel_table.flat,
        kernel_table.textured,
        kernel_table.strong_edge,
        kernel_table.microtexture,
        kernel_table.risky_halo_zone,
    ]
    .into_iter()
    .collect::<BTreeSet<_>>()
    .into_iter()
    .collect();
```

Replace with:

```rust
    // 2. Determine which distinct resize operations are needed (aliased
    //    kernels collapse to a single canonical form).
    let all_kernels: Vec<ResizeKernel> = [
        kernel_table.flat,
        kernel_table.textured,
        kernel_table.strong_edge,
        kernel_table.microtexture,
        kernel_table.risky_halo_zone,
    ]
    .into_iter()
    .map(canonical_kernel)
    .collect::<BTreeSet<_>>()
    .into_iter()
    .collect();
```

Then find the per-pixel selection:

```rust
            let region = region_map.get(sx, sy);
            let kernel = kernel_table.kernel_for(region);
```

Replace with:

```rust
            let region = region_map.get(sx, sy);
            let kernel = canonical_kernel(kernel_table.kernel_for(region));
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p r3sizer-core adaptive_dedups_mitchell -- --nocapture`
Expected: PASS.

- [ ] **Step 6: Run crate tests + clippy**

Run: `cargo test -p r3sizer-core && cargo clippy -p r3sizer-core -- -D warnings`
Expected: all green (existing `adaptive_resize_produces_valid_output` still passes; default table's Mitchell now folds into CatmullRom).

- [ ] **Step 7: Commit**

```bash
git add crates/r3sizer-core/src/resize_strategy.rs
git commit -m "fix(core): dedup aliased resize kernels in adaptive resize (#8)"
```

---

### Task 5: #3 — Include probing time in `full_total_us`

**Files:**
- Modify: `crates/r3sizer-core/src/pipeline.rs:990-997` (`full_total_us` sum)
- Test: append to the `#[cfg(test)] mod tests` created in Task 3

**Interfaces:**
- Consumes: `probing_us` (already in scope in `finish_pipeline`), `process_auto_sharp_downscale`.
- Produces: `timing.total_us` now includes probing time.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block at the end of `crates/r3sizer-core/src/pipeline.rs`:

```rust
    fn gradient(w: u32, h: u32) -> LinearRgbImage {
        let mut data = vec![0.0f32; (w * h * 3) as usize];
        for y in 0..h {
            for x in 0..w {
                let idx = ((y * w + x) * 3) as usize;
                data[idx] = x as f32 / w as f32;
                data[idx + 1] = y as f32 / h as f32;
                data[idx + 2] = 0.5;
            }
        }
        LinearRgbImage::new(w, h, data).unwrap()
    }

    #[test]
    fn total_time_includes_probing() {
        // 256x256 source so probing over ~11 probes is reliably > 1µs.
        let img = gradient(256, 256);
        let params = AutoSharpParams::photo(64, 64);
        let out = crate::process_auto_sharp_downscale(&img, &params).unwrap();
        let t = &out.diagnostics.timing;
        assert!(t.probing_us > 0, "probing_us should be measured");
        assert!(
            t.total_us >= t.probing_us,
            "total_us ({}) must include probing_us ({})",
            t.total_us,
            t.probing_us
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p r3sizer-core total_time_includes_probing -- --nocapture`
Expected: FAIL — `total_us` currently excludes probing.

- [ ] **Step 3: Add `probing_us` to the total**

In `crates/r3sizer-core/src/pipeline.rs`, find:

```rust
    let full_total_us = total_us
        + prepared.resize_us
        + prepared.base_quality_us
        + prepared.contrast_us
        + prepared.classification_us.unwrap_or(0)
        + prepared.baseline_us
        + prepared.evaluator_us.unwrap_or(0)
        + prepared.ingress_us.unwrap_or(0);
```

Replace with:

```rust
    let full_total_us = total_us
        + probing_us
        + prepared.resize_us
        + prepared.base_quality_us
        + prepared.contrast_us
        + prepared.classification_us.unwrap_or(0)
        + prepared.baseline_us
        + prepared.evaluator_us.unwrap_or(0)
        + prepared.ingress_us.unwrap_or(0);
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p r3sizer-core total_time_includes_probing -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Run crate tests + clippy**

Run: `cargo test -p r3sizer-core && cargo clippy -p r3sizer-core -- -D warnings`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add crates/r3sizer-core/src/pipeline.rs
git commit -m "fix(core): include probing time in total_us (#3)"
```

---

### Task 6: #4 — Sort externally-supplied probe samples defensively

**Files:**
- Modify: `crates/r3sizer-core/src/pipeline.rs:424-458` (`resolve_dense_strengths` — sort coarse copy + doc)
- Modify: `crates/r3sizer-core/src/pipeline.rs:692-696` (`finish_pipeline` — make `probe_samples` mutable and sort)
- Modify: `crates/r3sizer-core/src/pipeline.rs:384-391` (`process_from_prepared_with_probes` doc)
- Test: append to the `#[cfg(test)] mod tests` at the end of `crates/r3sizer-core/src/pipeline.rs`

**Interfaces:**
- Consumes: `resolve_dense_strengths`, `find_dense_window`, `ProbeSample`.
- Produces: both entry points tolerate unsorted input by sorting internally.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `crates/r3sizer-core/src/pipeline.rs`:

```rust
    fn sample(strength: f32, metric_value: f32) -> ProbeSample {
        ProbeSample {
            strength,
            artifact_ratio: metric_value,
            metric_value,
            breakdown: None,
        }
    }

    #[test]
    fn resolve_dense_strengths_tolerates_unsorted_input() {
        // TwoPass params so resolve_dense_strengths returns Some.
        let params = AutoSharpParams::photo(16, 16);

        // Crossing of p0=0.005 lies between strengths 0.5 (0.002) and 0.8 (0.010).
        let sorted = vec![
            sample(0.2, 0.001),
            sample(0.5, 0.002),
            sample(0.8, 0.010),
            sample(1.0, 0.020),
        ];
        let mut unsorted = sorted.clone();
        unsorted.swap(0, 3);
        unsorted.swap(1, 2);

        let a = resolve_dense_strengths(&sorted, &params, 0.005).unwrap();
        let b = resolve_dense_strengths(&unsorted, &params, 0.005).unwrap();
        // Same dense window regardless of input order.
        assert_eq!(a.unwrap().1.dense_min, b.unwrap().1.dense_min);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p r3sizer-core resolve_dense_strengths_tolerates_unsorted -- --nocapture`
Expected: FAIL — `find_dense_window` scans the unsorted slice and finds a different (or no) crossing.

- [ ] **Step 3: Sort inside `resolve_dense_strengths`**

In `crates/r3sizer-core/src/pipeline.rs`, find the `TwoPass` arm of `resolve_dense_strengths`:

```rust
            let (dense_lo, dense_hi) = find_dense_window(
                coarse_samples,
                effective_p0,
                *coarse_min,
                *coarse_max,
                *window_margin,
            );
```

Replace with:

```rust
            // Samples may arrive in any order (parallel probe pool); the
            // window search assumes ascending strength.
            let mut sorted = coarse_samples.to_vec();
            sorted.sort_by(|a, b| {
                a.strength
                    .partial_cmp(&b.strength)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let (dense_lo, dense_hi) = find_dense_window(
                &sorted,
                effective_p0,
                *coarse_min,
                *coarse_max,
                *window_margin,
            );
```

- [ ] **Step 4: Update `resolve_dense_strengths` doc**

Find the doc comment above `pub fn resolve_dense_strengths`:

```rust
/// Resolve the dense (second-pass) probe strengths from coarse results.
///
/// Only meaningful for [`ProbeConfig::TwoPass`].  Returns `Ok(None)` for other
/// configs (no second pass needed).
```

Replace with:

```rust
/// Resolve the dense (second-pass) probe strengths from coarse results.
///
/// Only meaningful for [`ProbeConfig::TwoPass`].  Returns `Ok(None)` for other
/// configs (no second pass needed).
///
/// `coarse_samples` may be supplied in any order (e.g. collected out of order
/// by the parallel probe pool); they are sorted internally by ascending
/// strength before the crossing window is located.
```

- [ ] **Step 5: Sort inside `finish_pipeline`**

Find the `ProbeResult` destructure in `finish_pipeline`:

```rust
    let ProbeResult {
        samples: probe_samples,
        pass_diagnostics: probe_pass_diagnostics,
        probing_us,
    } = probe_result;
```

Replace with:

```rust
    let ProbeResult {
        samples: mut probe_samples,
        pass_diagnostics: probe_pass_diagnostics,
        probing_us,
    } = probe_result;

    // Samples may arrive unsorted (externally-collected parallel probes).
    // Downstream fit/solve/monotonicity/window logic assumes ascending
    // strength, so sort defensively (idempotent for already-sorted input).
    probe_samples.sort_by(|a, b| {
        a.strength
            .partial_cmp(&b.strength)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
```

- [ ] **Step 6: Update `process_from_prepared_with_probes` doc**

Find the doc comment above `pub fn process_from_prepared_with_probes`:

```rust
/// Like [`process_from_prepared`] but with externally-collected probe samples.
///
/// Use this when probes were computed in parallel across multiple workers.
/// The `probing_us` field should reflect the wall-clock time of the parallel
/// probing phase (not the sum of per-worker times).
```

Replace with:

```rust
/// Like [`process_from_prepared`] but with externally-collected probe samples.
///
/// Use this when probes were computed in parallel across multiple workers.
/// The `probing_us` field should reflect the wall-clock time of the parallel
/// probing phase (not the sum of per-worker times).
///
/// `probe_samples` may be supplied in any order; they are sorted internally by
/// ascending strength before fitting and solving.
```

- [ ] **Step 7: Run test to verify it passes**

Run: `cargo test -p r3sizer-core resolve_dense_strengths_tolerates_unsorted -- --nocapture`
Expected: PASS.

- [ ] **Step 8: Run crate tests + clippy**

Run: `cargo test -p r3sizer-core && cargo clippy -p r3sizer-core -- -D warnings`
Expected: all green.

- [ ] **Step 9: Commit**

```bash
git add crates/r3sizer-core/src/pipeline.rs
git commit -m "fix(core): sort externally-supplied probe samples defensively (#4)"
```

---

### Task 7: `total_cmp` — make solver fallback ranking NaN-safe

**Files:**
- Modify: `crates/r3sizer-core/src/solve.rs:271,284,290,304,316,322` (six comparators)
- Test: `crates/r3sizer-core/src/solve.rs` (existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `find_sharpness_direct`, `ProbeSample`.
- Produces: `select_best_qualifying` / `select_least_bad` use `f32::total_cmp` (no panic on NaN).

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `crates/r3sizer-core/src/solve.rs`:

```rust
    #[test]
    fn direct_search_does_not_panic_on_nan_metric() {
        // One sample has a NaN metric_value (degenerate input). Ranking must
        // not panic (partial_cmp(...).unwrap() would).
        let samples = vec![
            ProbeSample { strength: 0.5, artifact_ratio: 0.001, metric_value: 0.001, breakdown: None },
            ProbeSample { strength: 1.0, artifact_ratio: f32::NAN, metric_value: f32::NAN, breakdown: None },
            ProbeSample { strength: 2.0, artifact_ratio: 0.002, metric_value: 0.002, breakdown: None },
            ProbeSample { strength: 3.0, artifact_ratio: 0.005, metric_value: 0.005, breakdown: None },
        ];
        let result = find_sharpness_direct(&samples, 0.003).unwrap();
        assert!(result.selected_strength.is_finite());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p r3sizer-core direct_search_does_not_panic_on_nan -- --nocapture`
Expected: FAIL — panics on `partial_cmp(...).unwrap()`.

- [ ] **Step 3: Swap the comparators**

In `crates/r3sizer-core/src/solve.rs`, replace each of these (they appear in `select_best_qualifying` and `select_least_bad`):

Replace both occurrences of:

```rust
                .max_by(|a, b| a.strength.partial_cmp(&b.strength).unwrap())
```

with:

```rust
                .max_by(|a, b| a.strength.total_cmp(&b.strength))
```

Replace both occurrences of:

```rust
                        ca.partial_cmp(&cb).unwrap()
```

with:

```rust
                        ca.total_cmp(&cb)
```

Replace both occurrences of:

```rust
                .min_by(|a, b| a.metric_value.partial_cmp(&b.metric_value).unwrap())
```

with:

```rust
                .min_by(|a, b| a.metric_value.total_cmp(&b.metric_value))
```

Note: `max_by` at line 290 and `min_by` at line 322 have the same surrounding indentation as their siblings; use `replace_all` semantics (all occurrences of each pattern).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p r3sizer-core direct_search_does_not_panic_on_nan -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Run crate tests + clippy**

Run: `cargo test -p r3sizer-core && cargo clippy -p r3sizer-core -- -D warnings`
Expected: all green (existing `solve.rs` policy tests unaffected — finite inputs order identically under `total_cmp`).

- [ ] **Step 6: Commit**

```bash
git add crates/r3sizer-core/src/solve.rs
git commit -m "fix(core): use total_cmp in solver fallback ranking (NaN-safe)"
```

---

### Task 8: #1 — Wire up `Hybrid`/`CompositeOnly` (policy-only gating)

**Files:**
- Modify: `crates/r3sizer-core/src/types.rs` (add `AutoSharpParams::needs_probe_breakdown`)
- Modify: `crates/r3sizer-core/src/pipeline.rs:18-30` (import `MetricWeights`)
- Modify: `crates/r3sizer-core/src/pipeline.rs` (`ProbeContext` struct + three builders + `probe_one_reuse`; `probe_strengths` and `run_two_pass_probing` signatures + call sites)
- Modify: `crates/r3sizer-core/src/recommendations.rs:367-371` (dormancy doc on `rule_switch_to_hybrid`)
- Modify: `CLAUDE.md:86` (per-probe breakdown wording)
- Test: `crates/r3sizer-core/src/pipeline.rs` `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `compute_metric_breakdown(sharpened, original, luma_original, luma_sharpened, artifact_metric, weights) -> MetricBreakdown`; `color::extract_luminance`; `run_probes_standalone`.
- Produces:
  - `AutoSharpParams::needs_probe_breakdown(&self) -> bool` (returns `selection_policy != GamutOnly`).
  - `ProbeContext` gains `need_breakdown: bool` and `metric_weights: MetricWeights`.
  - `fn probe_strengths(strengths, base, base_luminance, sharpen_mode, metric_mode, artifact_metric, baseline_artifact_ratio, kernel, metric_override, need_breakdown, metric_weights)`.
  - `fn run_two_pass_probing(coarse_count, coarse_min, coarse_max, dense_count, window_margin, p0, base, base_luminance, sharpen_mode, metric_mode, artifact_metric, baseline_artifact_ratio, kernel, metric_override, need_breakdown, metric_weights)`.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `crates/r3sizer-core/src/pipeline.rs`:

```rust
    #[test]
    fn probes_carry_breakdown_only_under_composite_policy() {
        use crate::SelectionPolicy;

        let base = gradient(24, 24);
        let luma = color::extract_luminance(&base);
        let strengths = [0.5f32, 1.0, 2.0, 3.0];

        // GamutOnly (default): fast path, no per-probe breakdown.
        let mut params = AutoSharpParams::photo(24, 24);
        params.selection_policy = SelectionPolicy::GamutOnly;
        let gamut = crate::run_probes_standalone(
            base.pixels(), base.width(), base.height(), &luma, &strengths, &params, 0.0,
        )
        .unwrap();
        assert!(gamut.iter().all(|s| s.breakdown.is_none()));

        // Hybrid: per-probe breakdown must be populated.
        params.selection_policy = SelectionPolicy::Hybrid;
        let hybrid = crate::run_probes_standalone(
            base.pixels(), base.width(), base.height(), &luma, &strengths, &params, 0.0,
        )
        .unwrap();
        assert!(hybrid.iter().all(|s| s.breakdown.is_some()));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p r3sizer-core probes_carry_breakdown_only_under_composite_policy -- --nocapture`
Expected: FAIL — the `Hybrid` assertion fails because probes always carry `breakdown: None`.

- [ ] **Step 3: Add `needs_probe_breakdown` to `AutoSharpParams`**

In `crates/r3sizer-core/src/types.rs`, inside `impl AutoSharpParams` (e.g. immediately after the `resolved` method), add:

```rust
    /// Whether the probe loop should compute a per-probe [`MetricBreakdown`].
    ///
    /// Only the composite-aware selection policies need it; the default
    /// `GamutOnly` path skips it to keep probing fast.
    pub(crate) fn needs_probe_breakdown(&self) -> bool {
        self.selection_policy != SelectionPolicy::GamutOnly
    }
```

- [ ] **Step 4: Import `MetricWeights` into pipeline.rs**

In `crates/r3sizer-core/src/pipeline.rs`, find:

```rust
    MetricMode, ProbeConfig, ProbePassDiagnostics, ProbeSample, ProcessOutput, RegionCoverage,
```

Replace with:

```rust
    MetricMode, MetricWeights, ProbeConfig, ProbePassDiagnostics, ProbeSample, ProcessOutput,
    RegionCoverage,
```

- [ ] **Step 5: Add fields to `ProbeContext`**

Find the `ProbeContext` struct:

```rust
/// Immutable context shared across all probes in a single probing phase.
struct ProbeContext<'a> {
    base: &'a LinearRgbImage,
    base_luminance: Option<&'a [f32]>,
    detail: &'a [f32],
    sharpen_mode: SharpenMode,
    artifact_metric: ArtifactMetric,
    baseline_artifact_ratio: f32,
    metric_mode: MetricMode,
    metric_override: Option<&'a (dyn Fn(&LinearRgbImage) -> f32 + Sync)>,
}
```

Replace with:

```rust
/// Immutable context shared across all probes in a single probing phase.
struct ProbeContext<'a> {
    base: &'a LinearRgbImage,
    base_luminance: Option<&'a [f32]>,
    detail: &'a [f32],
    sharpen_mode: SharpenMode,
    artifact_metric: ArtifactMetric,
    baseline_artifact_ratio: f32,
    metric_mode: MetricMode,
    metric_override: Option<&'a (dyn Fn(&LinearRgbImage) -> f32 + Sync)>,
    /// Compute a per-probe [`MetricBreakdown`] (composite policies / diagnostics).
    need_breakdown: bool,
    /// Weights for the composite score in the per-probe breakdown.
    metric_weights: MetricWeights,
}
```

- [ ] **Step 6: Populate breakdown in `probe_one_reuse`**

Find `probe_one_reuse` and replace its body from the `let p_total` line to the returned `ProbeSample`:

```rust
    let p_total = match ctx.metric_override {
        Some(f) => f(&scratch.rgb),
        None => crate::metrics::compute_selection_metric(&scratch.rgb, ctx.artifact_metric),
    };
    let metric_value = compute_metric_value(p_total, ctx.baseline_artifact_ratio, ctx.metric_mode);
    ProbeSample {
        strength,
        artifact_ratio: p_total,
        metric_value,
        breakdown: None,
    }
}
```

with:

```rust
    let p_total = match ctx.metric_override {
        Some(f) => f(&scratch.rgb),
        None => crate::metrics::compute_selection_metric(&scratch.rgb, ctx.artifact_metric),
    };
    let metric_value = compute_metric_value(p_total, ctx.baseline_artifact_ratio, ctx.metric_mode);

    let breakdown = if ctx.need_breakdown {
        // luma_sharpened: reuse scratch.luma in Lightness mode; extract in RGB mode.
        let sharp_luma_owned;
        let sharp_luma: &[f32] = match ctx.sharpen_mode {
            SharpenMode::Lightness => &scratch.luma,
            SharpenMode::Rgb => {
                sharp_luma_owned = color::extract_luminance(&scratch.rgb);
                &sharp_luma_owned
            }
        };
        // luma_original: prefer precomputed base luminance, else extract.
        let base_luma_owned;
        let base_luma: &[f32] = match ctx.base_luminance {
            Some(l) => l,
            None => {
                base_luma_owned = color::extract_luminance(ctx.base);
                &base_luma_owned
            }
        };
        Some(crate::metrics::compute_metric_breakdown(
            &scratch.rgb,
            ctx.base,
            base_luma,
            sharp_luma,
            ctx.artifact_metric,
            &ctx.metric_weights,
        ))
    } else {
        None
    };

    ProbeSample {
        strength,
        artifact_ratio: p_total,
        metric_value,
        breakdown,
    }
}
```

- [ ] **Step 7: Thread the two fields through `probe_strengths`**

Find the `probe_strengths` signature:

```rust
#[allow(clippy::too_many_arguments)]
fn probe_strengths(
    strengths: &[f32],
    base: &LinearRgbImage,
    base_luminance: Option<&[f32]>,
    sharpen_mode: SharpenMode,
    metric_mode: MetricMode,
    artifact_metric: ArtifactMetric,
    baseline_artifact_ratio: f32,
    kernel: &[f32],
    metric_override: Option<&(dyn Fn(&LinearRgbImage) -> f32 + Sync)>,
) -> Result<Vec<ProbeSample>, CoreError> {
```

Replace with:

```rust
#[allow(clippy::too_many_arguments)]
fn probe_strengths(
    strengths: &[f32],
    base: &LinearRgbImage,
    base_luminance: Option<&[f32]>,
    sharpen_mode: SharpenMode,
    metric_mode: MetricMode,
    artifact_metric: ArtifactMetric,
    baseline_artifact_ratio: f32,
    kernel: &[f32],
    metric_override: Option<&(dyn Fn(&LinearRgbImage) -> f32 + Sync)>,
    need_breakdown: bool,
    metric_weights: MetricWeights,
) -> Result<Vec<ProbeSample>, CoreError> {
```

Then find the `ProbeContext` built inside `probe_strengths`:

```rust
    let ctx = ProbeContext {
        base,
        base_luminance,
        detail: &detail,
        sharpen_mode,
        artifact_metric,
        baseline_artifact_ratio,
        metric_mode,
        metric_override,
    };
```

Replace with:

```rust
    let ctx = ProbeContext {
        base,
        base_luminance,
        detail: &detail,
        sharpen_mode,
        artifact_metric,
        baseline_artifact_ratio,
        metric_mode,
        metric_override,
        need_breakdown,
        metric_weights,
    };
```

- [ ] **Step 8: Thread the two fields through `run_two_pass_probing`**

Find the `run_two_pass_probing` signature:

```rust
#[allow(clippy::too_many_arguments)]
fn run_two_pass_probing(
    coarse_count: usize,
    coarse_min: f32,
    coarse_max: f32,
    dense_count: usize,
    window_margin: f32,
    p0: f32,
    base: &LinearRgbImage,
    base_luminance: Option<&[f32]>,
    sharpen_mode: SharpenMode,
    metric_mode: MetricMode,
    artifact_metric: ArtifactMetric,
    baseline_artifact_ratio: f32,
    kernel: &[f32],
    metric_override: Option<&(dyn Fn(&LinearRgbImage) -> f32 + Sync)>,
) -> Result<(Vec<ProbeSample>, Option<ProbePassDiagnostics>), CoreError> {
```

Replace with (add the two params before the closing paren):

```rust
#[allow(clippy::too_many_arguments)]
fn run_two_pass_probing(
    coarse_count: usize,
    coarse_min: f32,
    coarse_max: f32,
    dense_count: usize,
    window_margin: f32,
    p0: f32,
    base: &LinearRgbImage,
    base_luminance: Option<&[f32]>,
    sharpen_mode: SharpenMode,
    metric_mode: MetricMode,
    artifact_metric: ArtifactMetric,
    baseline_artifact_ratio: f32,
    kernel: &[f32],
    metric_override: Option<&(dyn Fn(&LinearRgbImage) -> f32 + Sync)>,
    need_breakdown: bool,
    metric_weights: MetricWeights,
) -> Result<(Vec<ProbeSample>, Option<ProbePassDiagnostics>), CoreError> {
```

Then find the `ProbeContext` built inside `run_two_pass_probing`:

```rust
    let ctx = ProbeContext {
        base,
        base_luminance,
        detail: &detail,
        sharpen_mode,
        artifact_metric,
        baseline_artifact_ratio,
        metric_mode,
        metric_override,
    };
    let mut scratch = ProbeScratch {
```

Replace with:

```rust
    let ctx = ProbeContext {
        base,
        base_luminance,
        detail: &detail,
        sharpen_mode,
        artifact_metric,
        baseline_artifact_ratio,
        metric_mode,
        metric_override,
        need_breakdown,
        metric_weights,
    };
    let mut scratch = ProbeScratch {
```

- [ ] **Step 9: Set the two fields in the `run_probes_from_detail` context**

Find the `ProbeContext` built inside `run_probes_from_detail`:

```rust
    let ctx = ProbeContext {
        base: &base,
        base_luminance: Some(base_luminance),
        detail,
        sharpen_mode: params.sharpen_mode,
        artifact_metric: params.artifact_metric,
        baseline_artifact_ratio,
        metric_mode: params.metric_mode,
        metric_override: None,
    };
```

Replace with:

```rust
    let ctx = ProbeContext {
        base: &base,
        base_luminance: Some(base_luminance),
        detail,
        sharpen_mode: params.sharpen_mode,
        artifact_metric: params.artifact_metric,
        baseline_artifact_ratio,
        metric_mode: params.metric_mode,
        metric_override: None,
        need_breakdown: params.needs_probe_breakdown(),
        metric_weights: params.metric_weights,
    };
```

- [ ] **Step 10: Update the `probe_strengths` call in `run_probes_standalone`**

Find:

```rust
    probe_strengths(
        strengths,
        &base,
        Some(base_luminance),
        params.sharpen_mode,
        params.metric_mode,
        params.artifact_metric,
        baseline_artifact_ratio,
        &kernel,
        None,
    )
```

Replace with:

```rust
    probe_strengths(
        strengths,
        &base,
        Some(base_luminance),
        params.sharpen_mode,
        params.metric_mode,
        params.artifact_metric,
        baseline_artifact_ratio,
        &kernel,
        None,
        params.needs_probe_breakdown(),
        params.metric_weights,
    )
```

- [ ] **Step 11: Update the `run_two_pass_probing` and `probe_strengths` calls in `run_probes_for_prepared`**

Find the `run_two_pass_probing(` call (the `ProbeConfig::TwoPass` arm):

```rust
        ) => run_two_pass_probing(
            *coarse_count,
            *coarse_min,
            *coarse_max,
            *dense_count,
            *window_margin,
            effective_p0,
            base,
            base_luminance,
            params.sharpen_mode,
            params.metric_mode,
            params.artifact_metric,
            baseline_artifact_ratio,
            &kernel,
            metric_override,
        )?,
```

Replace with:

```rust
        ) => run_two_pass_probing(
            *coarse_count,
            *coarse_min,
            *coarse_max,
            *dense_count,
            *window_margin,
            effective_p0,
            base,
            base_luminance,
            params.sharpen_mode,
            params.metric_mode,
            params.artifact_metric,
            baseline_artifact_ratio,
            &kernel,
            metric_override,
            params.needs_probe_breakdown(),
            params.metric_weights,
        )?,
```

Then find the `probe_strengths(` call in the `_ =>` arm:

```rust
            let samples = probe_strengths(
                &strengths,
                base,
                base_luminance,
                params.sharpen_mode,
                params.metric_mode,
                params.artifact_metric,
                baseline_artifact_ratio,
                &kernel,
                metric_override,
            )?;
```

Replace with:

```rust
            let samples = probe_strengths(
                &strengths,
                base,
                base_luminance,
                params.sharpen_mode,
                params.metric_mode,
                params.artifact_metric,
                baseline_artifact_ratio,
                &kernel,
                metric_override,
                params.needs_probe_breakdown(),
                params.metric_weights,
            )?;
```

- [ ] **Step 12: Run the test to verify it passes**

Run: `cargo test -p r3sizer-core probes_carry_breakdown_only_under_composite_policy -- --nocapture`
Expected: PASS.

- [ ] **Step 13: Document `rule_switch_to_hybrid` dormancy**

In `crates/r3sizer-core/src/recommendations.rs`, find the doc comment above `fn rule_switch_to_hybrid`:

```rust
/// Recommend Hybrid selection policy when GamutOnly fallback chose a sample
/// that a composite-aware ranking would have replaced.
///
/// Only fires when the solver used fallback (BestSampleWithinBudget or
/// LeastBadSample) — polynomial root selection is identical across policies.
```

Replace with:

```rust
/// Recommend Hybrid selection policy when GamutOnly fallback chose a sample
/// that a composite-aware ranking would have replaced.
///
/// Only fires when the solver used fallback (BestSampleWithinBudget or
/// LeastBadSample) — polynomial root selection is identical across policies.
///
/// **Dormant by design under policy-only breakdown gating:** per-probe
/// `MetricBreakdown`s are only computed when `selection_policy != GamutOnly`,
/// but this rule only runs while the current policy *is* `GamutOnly`.  The two
/// never overlap, so with the current gating this rule produces no output.  It
/// stays in place for when per-probe breakdowns become available under
/// `GamutOnly` (e.g. a future opt-in).
```

- [ ] **Step 14: Correct the CLAUDE.md wording**

In `CLAUDE.md`, find:

```
- **All four composite metric components are active** — `MetricBreakdown` with `MetricComponent` variants (GamutExcursion, HaloRinging, EdgeOvershoot, TextureFlattening) is populated per probe. The `aggregate` field preserves backward compatibility with the scalar fitting path. Configurable weights via `MetricWeights` (default: 1.0, 0.3, 0.3, 0.1).
```

Replace with:

```
- **All four composite metric components are active** — `MetricBreakdown` with `MetricComponent` variants (GamutExcursion, HaloRinging, EdgeOvershoot, TextureFlattening) is populated per probe **when `selection_policy != GamutOnly`** (the composite-aware policies need it; the default `GamutOnly` path skips it to keep probing fast), and always at final measurement. The `aggregate` field preserves backward compatibility with the scalar fitting path. Configurable weights via `MetricWeights` (default: 1.0, 0.3, 0.3, 0.1).
```

- [ ] **Step 15: Run crate tests + clippy**

Run: `cargo test -p r3sizer-core && cargo clippy -p r3sizer-core -- -D warnings`
Expected: all green (existing `solve.rs` Hybrid tests already validate composite ranking once breakdowns exist).

- [ ] **Step 16: Commit**

```bash
git add crates/r3sizer-core/src/types.rs crates/r3sizer-core/src/pipeline.rs crates/r3sizer-core/src/recommendations.rs CLAUDE.md
git commit -m "feat(core): compute per-probe breakdown for composite policies (#1)"
```

---

### Task 9: #2 — Evaluator cap as a recorded advisory selection stage

**Files:**
- Modify: `crates/r3sizer-core/src/types.rs` (add `EvaluatorCapDiagnostics`; add `evaluator_cap` field to `AutoSharpDiagnostics`)
- Modify: `crates/r3sizer-core/src/lib.rs:44-64` (export `EvaluatorCapDiagnostics`)
- Modify: `crates/r3sizer-core/src/pipeline.rs` (import `EvaluatorCapDiagnostics`; add `apply_evaluator_cap` helper; use it in `finish_pipeline`; set `evaluator_cap` in the diagnostics literal)
- Modify: `crates/r3sizer-core/src/evaluator.rs:1-11,20-24` (reframe docs)
- Modify: `CLAUDE.md:55` (evaluator module-list line)
- Modify: `crates/r3sizer-core/tests/typegen.rs` (add `EvaluatorCapDiagnostics` to imports + declarations; fix `out_path`)
- Regenerate: `web/src/shared/types/generated.ts`
- Test: `crates/r3sizer-core/src/pipeline.rs` `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: `prepared.evaluator_cap: Option<f32>`, `solve_result.selected_strength: f32`.
- Produces:
  - `struct EvaluatorCapDiagnostics { cap: f32, strength_before_cap: f32 }`.
  - `fn apply_evaluator_cap(solved_strength: f32, cap: Option<f32>) -> (f32, Option<EvaluatorCapDiagnostics>)`.
  - `AutoSharpDiagnostics.evaluator_cap: Option<EvaluatorCapDiagnostics>`.

- [ ] **Step 1: Add the `EvaluatorCapDiagnostics` type**

In `crates/r3sizer-core/src/types.rs`, immediately before `pub struct QualityEvaluation {`, add:

```rust
/// Records that the advisory quality evaluator lowered the final sharpening
/// strength below the solver's selected value.
///
/// Present only when the evaluator's strength cap actually bound; `None` when
/// the solver's strength was already at or below the cap, or the evaluator was
/// disabled.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct EvaluatorCapDiagnostics {
    /// The evaluator's suggested strength ceiling.
    pub cap: f32,
    /// The solver's selected strength before the cap was applied.
    pub strength_before_cap: f32,
}
```

- [ ] **Step 2: Add the diagnostics field**

In `crates/r3sizer-core/src/types.rs`, in `struct AutoSharpDiagnostics`, find:

```rust
    /// Quality evaluator result (advisory).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluator_result: Option<QualityEvaluation>,
```

Replace with:

```rust
    /// Quality evaluator result (advisory).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluator_result: Option<QualityEvaluation>,

    /// Set when the evaluator's advisory strength cap lowered the final s\*.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluator_cap: Option<EvaluatorCapDiagnostics>,
```

- [ ] **Step 3: Export the type from lib.rs**

In `crates/r3sizer-core/src/lib.rs`, find (in the `pub use types::{ ... }` block):

```rust
    DiagnosticsLevel, EvaluationColorSpace, EvaluatorConfig, ExperimentalSharpenMode,
```

Replace with:

```rust
    DiagnosticsLevel, EvaluationColorSpace, EvaluatorCapDiagnostics, EvaluatorConfig,
    ExperimentalSharpenMode,
```

- [ ] **Step 4: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `crates/r3sizer-core/src/pipeline.rs`:

```rust
    #[test]
    fn evaluator_cap_records_when_it_binds() {
        use crate::EvaluatorCapDiagnostics;

        // Cap below solver strength → capped + recorded.
        let (s, diag) = apply_evaluator_cap(1.0, Some(0.25));
        assert!((s - 0.25).abs() < 1e-6);
        let d: EvaluatorCapDiagnostics = diag.expect("cap should be recorded");
        assert!((d.cap - 0.25).abs() < 1e-6);
        assert!((d.strength_before_cap - 1.0).abs() < 1e-6);

        // Cap at/above solver strength → no change, no record.
        let (s, diag) = apply_evaluator_cap(0.2, Some(0.25));
        assert!((s - 0.2).abs() < 1e-6);
        assert!(diag.is_none());

        // No cap → no change, no record.
        let (s, diag) = apply_evaluator_cap(1.0, None);
        assert!((s - 1.0).abs() < 1e-6);
        assert!(diag.is_none());
    }
```

- [ ] **Step 5: Run test to verify it fails**

Run: `cargo test -p r3sizer-core evaluator_cap_records_when_it_binds -- --nocapture`
Expected: FAIL to compile — `apply_evaluator_cap` does not exist yet.

- [ ] **Step 6: Add the `apply_evaluator_cap` helper and import**

In `crates/r3sizer-core/src/pipeline.rs`, add `EvaluatorCapDiagnostics` to the crate import block. Find:

```rust
    CoreError, DiagnosticsLevel, FallbackReason, FitStatus, FitStrategy, ImageSize, LinearRgbImage,
```

Replace with:

```rust
    CoreError, DiagnosticsLevel, EvaluatorCapDiagnostics, FallbackReason, FitStatus, FitStrategy,
    ImageSize, LinearRgbImage,
```

Then add the helper immediately above `fn finish_pipeline`:

```rust
/// Apply the evaluator's advisory strength cap and record it when it binds.
///
/// The evaluator is an advisory *selection* stage: it may lower the final
/// strength below the solver's value.  Returns the (possibly capped) strength
/// and a diagnostic record that is `Some` only when the cap actually bound.
fn apply_evaluator_cap(
    solved_strength: f32,
    cap: Option<f32>,
) -> (f32, Option<EvaluatorCapDiagnostics>) {
    match cap {
        Some(c) if solved_strength > c => (
            c,
            Some(EvaluatorCapDiagnostics {
                cap: c,
                strength_before_cap: solved_strength,
            }),
        ),
        _ => (solved_strength, None),
    }
}
```

- [ ] **Step 7: Use the helper in `finish_pipeline`**

Find:

```rust
    // Evaluator cap
    let selected_strength = match prepared.evaluator_cap {
        Some(cap) if solve_result.selected_strength > cap => cap,
        _ => solve_result.selected_strength,
    };
```

Replace with:

```rust
    // Evaluator cap — advisory selection stage; recorded when it binds.
    let (selected_strength, evaluator_cap_diag) =
        apply_evaluator_cap(solve_result.selected_strength, prepared.evaluator_cap);
```

- [ ] **Step 8: Set the field in the diagnostics literal**

In `finish_pipeline`, find the diagnostics construction line:

```rust
        evaluator_result: _evaluator_result,
        recommendations: Vec::new(),
```

Replace with:

```rust
        evaluator_result: _evaluator_result,
        evaluator_cap: evaluator_cap_diag,
        recommendations: Vec::new(),
```

- [ ] **Step 9: Run the test to verify it passes**

Run: `cargo test -p r3sizer-core evaluator_cap_records_when_it_binds -- --nocapture`
Expected: PASS.

- [ ] **Step 10: Reframe the evaluator docs**

In `crates/r3sizer-core/src/evaluator.rs`, find the module doc lines:

```rust
//! Branch A: defines a `QualityEvaluator` trait and a hand-crafted
//! `HeuristicEvaluator` implementation. The evaluator is purely
//! diagnostic — it does not alter the pipeline's s* selection.
```

Replace with:

```rust
//! Branch A: defines a `QualityEvaluator` trait and a hand-crafted
//! `HeuristicEvaluator` implementation. The evaluator has two roles: an
//! advisory strength cap that can lower the final s* (a genuine selection
//! stage, recorded in `AutoSharpDiagnostics::evaluator_cap` when it binds) and
//! a post-hoc quality evaluation that is purely diagnostic.
```

Then find the trait doc:

```rust
/// Quality evaluator interface.
///
/// Designed to be object-safe for dynamic dispatch, but the pipeline
/// currently uses static dispatch via `HeuristicEvaluator`.
```

Replace with:

```rust
/// Quality evaluator interface.
///
/// Designed to be object-safe for dynamic dispatch, but the pipeline
/// currently uses static dispatch via `HeuristicEvaluator`.  `suggest_strength`
/// feeds an advisory cap that can lower the final s*; `evaluate` produces a
/// post-hoc, diagnostic-only quality score.
```

- [ ] **Step 11: Update the `EvaluatorConfig` doc**

In `crates/r3sizer-core/src/types.rs`, find:

```rust
/// Configuration for the quality evaluator.
///
/// The evaluator runs after final sharpening and produces advisory diagnostics.
/// It does **not** alter the pipeline's s* selection.
```

Replace with:

```rust
/// Configuration for the quality evaluator.
///
/// The evaluator contributes an advisory strength cap that can lower the final
/// s* (recorded in [`AutoSharpDiagnostics::evaluator_cap`] when it binds) and a
/// post-hoc quality evaluation that is diagnostic-only.
```

- [ ] **Step 12: Update the CLAUDE.md evaluator line**

In `CLAUDE.md`, find:

```
- `evaluator.rs` — heuristic quality evaluator (feature extraction + advisory strength cap)
```

Replace with:

```
- `evaluator.rs` — heuristic quality evaluator: an advisory strength cap that can lower the final s\* (recorded in `AutoSharpDiagnostics::evaluator_cap` when it binds) plus a diagnostic-only post-sharpen quality score
```

- [ ] **Step 13: Register `EvaluatorCapDiagnostics` in the typegen test and fix its output path**

In `crates/r3sizer-core/tests/typegen.rs`, find (in the `use r3sizer_core::{ ... }` import list):

```rust
    FallbackReason, FitQuality, FitStatus, FitStrategy, GainTable, ImageFeatures, ImageSize,
```

Replace with:

```rust
    EvaluatorCapDiagnostics, FallbackReason, FitQuality, FitStatus, FitStrategy, GainTable,
    ImageFeatures, ImageSize,
```

Then find (in the extended declarations `d.extend(vec![ ... ])`):

```rust
            EvaluatorConfig::decl(&cfg),
            ImageFeatures::decl(&cfg),
            QualityEvaluation::decl(&cfg),
```

Replace with:

```rust
            EvaluatorConfig::decl(&cfg),
            EvaluatorCapDiagnostics::decl(&cfg),
            ImageFeatures::decl(&cfg),
            QualityEvaluation::decl(&cfg),
```

Then find the output path:

```rust
    let out_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/src/types/generated.ts");
```

Replace with:

```rust
    let out_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../web/src/shared/types/generated.ts");
```

Also update the doc comment at the top of the file:

```rust
//! Output: web/src/types/generated.ts
```

Replace with:

```rust
//! Output: web/src/shared/types/generated.ts
```

- [ ] **Step 14: Regenerate the TypeScript bindings**

Run: `cargo test -p r3sizer-core --features typegen export_typescript_bindings -- --nocapture`
Expected: prints `✓ Wrote .../web/src/shared/types/generated.ts`. Confirm the new type is present:

Run: `grep -n "EvaluatorCapDiagnostics\|evaluator_cap" web/src/shared/types/generated.ts`
Expected: at least one `type EvaluatorCapDiagnostics = ...` line and an `evaluator_cap` field on `AutoSharpDiagnostics`.

- [ ] **Step 15: Run crate tests + workspace clippy**

Run: `cargo test -p r3sizer-core && cargo clippy --workspace -- -D warnings`
Expected: all green.

- [ ] **Step 16: Commit**

```bash
git add crates/r3sizer-core/src/types.rs crates/r3sizer-core/src/lib.rs crates/r3sizer-core/src/pipeline.rs crates/r3sizer-core/src/evaluator.rs crates/r3sizer-core/tests/typegen.rs CLAUDE.md web/src/shared/types/generated.ts
git commit -m "feat(core): record evaluator strength cap as advisory selection stage (#2)"
```

---

### Task 10: Final verification

**Files:** none (verification only).

- [ ] **Step 1: Full workspace test + clippy**

Run: `cargo test --workspace && cargo clippy --workspace -- -D warnings`
Expected: all tests pass, no clippy warnings.

- [ ] **Step 2: Confirm the spec is fully covered**

Review `docs/superpowers/specs/2026-07-06-core-review-findings-design.md` against the commit log:

Run: `git log --oneline main..HEAD`
Expected: one commit per finding (#7, #6, #5, #8, #3, #4, total_cmp, #1, #2) plus the design/plan doc commits.
