# Stamped probe-worker seam Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the parallel-probing seam reject wrong-length buffers and refuse probe batches not measured on the base and params being finished (ticket #42).

**Architecture:** A `ProbeSettings` value holds exactly what a probe reads, and both probe paths measure through it. `PreparedBase` mints a `ProbeProvenance` (base instance, baseline, settings); `ProbeWorkerInput` is built only from a `PreparedBase` or through one validating constructor, and stamps every batch it produces. `process_from_prepared_with_probes` checks the stamp first. Wasm and web carry the stamp as an opaque JSON string.

**Tech Stack:** Rust (`r3sizer-core`, `r3sizer-wasm` with `wasm-bindgen`), TypeScript (web worker pool, vitest).

**Spec:** `docs/superpowers/specs/2026-11-25-probe-seam-provenance-design.md`

## Global Constraints

- Lengths are exact: `base_luminance.len() == W × H` in both modes; `detail.len() == W × H` in Lightness mode and `W × H × 3` in Rgb mode. Errors are `CoreError::BufferLengthMismatch { expected_len, got_len }`.
- A refused stamp is `CoreError::ProbeProvenanceMismatch`, displaying `probe batch was not measured on this prepared base with these params`.
- No new dependency in `r3sizer-core` (no hasher, no `serde_json`). Floats are compared as `f32::to_bits`.
- Web deduplication keeps today's rule: samples closer than `1e-5` in strength are merged.
- JS never reads inside a provenance. It compares whole provenance objects and forwards the string.
- `web/src/shared/api/processing/wasm-pkg/` is build output and is never committed. `generated.ts` must stay unchanged (no serializable type changes).
- `pipeline` and `types` are Stable-tier, pre-1.0: API removals and signature changes go in the CHANGELOG and the PR body.
- Commit style: imperative subject, at most 72 characters, body says why.
- Serial results must not change. The Appendix harness must report 0 differing lines after Tasks 2, 3 and 5.

## Review Focus

1. A garbage or truncated `provenance_json` reaching a wasm entry point must give a typed error, never a panic or a default stamp. Pinned in Task 4 (`a_worker_rejects_wrong_buffers_and_foreign_stamps`).
2. A param edited between probing and finishing that only affects the finish (`fit_strategy`, `diagnostics_level`) must leave the batch accepted. Pinned in Task 3 (`the_finish_refuses_a_batch_whose_probe_settings_differ`).
3. The same base prepared again between probing and finishing must be refused as a new instance. Pinned in Task 3 (`a_probe_batch_is_refused_by_another_instance_of_the_same_base`) and Task 4 (`process_from_probes_refuses_a_stale_batch`).
4. `run_probes(&[])` must give an empty batch that still carries the stamp. Pinned in Task 3 (`probing_no_strengths_gives_an_empty_stamped_batch`).
5. One worker still holding the previous base must not slip its batch into a round of fresh ones. Pinned in Task 4 (`mergeProbeBatches` tests).

## File Structure

- `crates/r3sizer-core/src/pipeline.rs` — all new types and the finish check; the measurement path stays private to this file.
- `crates/r3sizer-core/src/types.rs` — `SelectionPolicy::needs_probe_breakdown`. `lib.rs` — re-exports and the new `CoreError` variant.
- `crates/r3sizer-core/tests/integration.rs` — seam behavior through the public API.
- `crates/r3sizer-wasm/src/lib.rs` — the three stamp-aware exports. `tests/probe_seam.rs` — new; the seam under node.
- `web/src/shared/api/processing/probe-worker.ts` — `BaseData` lives here; the one wasm call, exported as `runProbeBatch`. `probe-pool.ts` — `mergeProbeBatches`, `runProbesParallel`. `wasm.ts`, `wasm-worker.ts` — the minted bundle and the stamped batch. Two new `.test.ts` files beside them.
- `CHANGELOG.md`, `CLAUDE.md`, `docs/architecture.md`, `docs/future_work.md`.

---

### Task 1: Pin which `AutoSharpParams` fields affect the base

**Files:** Modify `crates/r3sizer-core/src/pipeline.rs` (`mod tests`, after `resolve_dense_strengths_tolerates_unsorted_input`). No production change.

**Interfaces:** Produces (test-only; Tasks 2 and 3 reuse them): `BASE_AFFECTING: [&str; 10]`, `PROBE_ONLY: [&str; 5]`, `NEITHER: [&str; 6]`, `fn toggle_param(name: &str, params: &mut AutoSharpParams)`.

- [ ] **Step 1: Add the role lists and the compile-time tripwire**

```rust
    // -----------------------------------------------------------------------
    // AutoSharpParams field roles
    // -----------------------------------------------------------------------

    /// Fields that change the [`PreparedBase`]; [`BaseParamsKey`] must cover
    /// exactly these.  `target` is one key field holding the first two.
    const BASE_AFFECTING: [&str; 10] = [
        "target_width", "target_height", "target_artifact_ratio", "enable_contrast_leveling",
        "sharpen_strategy", "input_color_space", "resize_strategy", "evaluation_color_space",
        "artifact_metric", "evaluator_config",
    ];
    /// Fields only the probe measurement reads.
    const PROBE_ONLY: [&str; 5] =
        ["sharpen_sigma", "sharpen_mode", "metric_mode", "metric_weights", "selection_policy"];
    /// Fields read by neither the base nor the probes.
    const NEITHER: [&str; 6] = [
        "probe_strengths", "fit_strategy", "output_clamp", "diagnostics_level",
        "experimental_sharpen_mode", "pipeline_mode",
    ];

    /// Compile-time tripwire for the three role lists.  The destructure has no
    /// `..`, so a new field fails to compile until it is listed; a binding left
    /// out of every array is an unused-variable error under `-D warnings`.
    #[test]
    fn every_param_field_has_a_role() {
        let AutoSharpParams {
            target_width, target_height, probe_strengths, target_artifact_ratio,
            enable_contrast_leveling, sharpen_sigma, fit_strategy, output_clamp, sharpen_mode,
            metric_mode, artifact_metric, metric_weights, selection_policy, diagnostics_level,
            sharpen_strategy, input_color_space, resize_strategy, experimental_sharpen_mode,
            evaluation_color_space, evaluator_config, pipeline_mode,
        } = AutoSharpParams::default();
        // One `&dyn Debug` per binding, grouped and ordered as the constants name them.
        let base_affecting: [&dyn std::fmt::Debug; BASE_AFFECTING.len()] = [
            &target_width, &target_height, &target_artifact_ratio, &enable_contrast_leveling,
            &sharpen_strategy, &input_color_space, &resize_strategy, &evaluation_color_space,
            &artifact_metric, &evaluator_config,
        ];
        let probe_only: [&dyn std::fmt::Debug; PROBE_ONLY.len()] = [
            &sharpen_sigma, &sharpen_mode, &metric_mode, &metric_weights, &selection_policy,
        ];
        let neither: [&dyn std::fmt::Debug; NEITHER.len()] = [
            &probe_strengths, &fit_strategy, &output_clamp, &diagnostics_level,
            &experimental_sharpen_mode, &pipeline_mode,
        ];
        assert_eq!(base_affecting.len() + probe_only.len() + neither.len(), 21);
    }
```

- [ ] **Step 2: Add `toggle_param` and the two table tests**

`toggle_param(name, params)` assigns, by name:

- `target_width`, `target_height`: `+ 7`
- `target_artifact_ratio`: `0.5`, or `0.25` when it is already `0.5`
- `enable_contrast_leveling`: negated
- `sharpen_strategy`: `Uniform` and `ContentAdaptive { classification: ClassificationParams::default(), gain_table: GainTable::v03_default(), max_backoff_iterations: 4, backoff_scale_factor: 0.8 }`, each to the other
- `input_color_space`: `None` and `Some(InputColorSpace::LinearRgb)`, each to the other
- `resize_strategy`: `None` and `Some(ResizeStrategy::Uniform { kernel: ResizeKernel::CatmullRom })`, each to the other
- `evaluation_color_space`: `None` and `Some(EvaluationColorSpace::LumaOnly)`, each to the other
- `artifact_metric`, `sharpen_mode`, `metric_mode`, `fit_strategy`, `output_clamp`, `diagnostics_level`: the other variant
- `evaluator_config`: `None` and `Some(EvaluatorConfig::Heuristic)`, each to the other
- `sharpen_sigma`: `1.5`
- `metric_weights`: `gamut_excursion = 2.0`
- `selection_policy`: `GamutOnly` and `Hybrid`, each to the other
- `probe_strengths`: `ProbeConfig::Explicit(vec![0.25, 0.5, 1.0, 2.0])`
- `experimental_sharpen_mode`: `None` and `Some(ExperimentalSharpenMode::LumaPlusChromaGuard { max_chroma_shift: 0.25, chroma_region_factors: None, saturation_guard: None })`, each to the other
- `pipeline_mode`: `None` and `Some(PipelineMode::Fast)`, each to the other
- any other name: `panic!("`{name}` is not in any role list")`

Match on `*params.<field>` without moving: the composite fields are not `Copy`.

```rust
    fn prepared_photo() -> PreparedBase {
        prepare_base(&gradient(48, 36), &AutoSharpParams::photo(16, 12), &|_| {}).unwrap()
    }

    #[test]
    fn base_params_key_rejects_a_change_in_any_base_affecting_field() {
        let prepared = prepared_photo();
        for name in BASE_AFFECTING {
            let mut changed = AutoSharpParams::photo(16, 12);
            toggle_param(name, &mut changed);
            assert!(!prepared.matches_params(&changed), "`{name}` changes the prepared base but BaseParamsKey ignores it");
        }
    }

    #[test]
    fn base_params_key_ignores_every_field_that_leaves_the_base_unchanged() {
        let prepared = prepared_photo();
        assert!(prepared.matches_params(&AutoSharpParams::photo(16, 12)));
        for name in PROBE_ONLY.into_iter().chain(NEITHER) {
            let mut changed = AutoSharpParams::photo(16, 12);
            toggle_param(name, &mut changed);
            assert!(prepared.matches_params(&changed), "`{name}` leaves the prepared base unchanged but BaseParamsKey rejects it");
        }
    }
```

- [ ] **Step 3: Run**

Run: `cargo test -p r3sizer-core --lib base_params_key every_param_field_has_a_role`
Expected: PASS. This task adds no production code; the tests pin behavior that holds today, so Steps 4 and 5 prove each can fail.

- [ ] **Step 4: Mutation check A**

In `BaseParamsKey::from_params`, temporarily change `evaluator_config: params.evaluator_config,` to `evaluator_config: None,`.
Run: `cargo test -p r3sizer-core --lib base_params_key_rejects`
Expected: FAIL with `` `evaluator_config` changes the prepared base but BaseParamsKey ignores it ``. Revert.

- [ ] **Step 5: Mutation check B**

Temporarily add `pub scratch: bool,` to `AutoSharpParams` and `scratch: false,` to its `Default` impl.
Run: `cargo test -p r3sizer-core --lib --no-run`
Expected: compile error E0027, `` pattern does not mention field `scratch` ``, at `every_param_field_has_a_role`. Revert.

- [ ] **Step 6: Commit**

```bash
git add crates/r3sizer-core/src/pipeline.rs
git commit -m "test: pin which AutoSharpParams fields affect the prepared base"
```

---

### Task 2: One `ProbeSettings` for both probe paths

**Files:** Modify `crates/r3sizer-core/src/pipeline.rs` (`ProbeContext` ~1805, `probe_one_reuse` ~1831, `probe_strengths` ~1216, `run_two_pass_probing` ~1706, `run_probes_for_prepared` ~598, `run_probes_standalone` ~482, `run_probes_from_detail` ~549, `mod tests`), `types.rs` (`needs_probe_breakdown` ~671, `SelectionPolicy` ~174), `lib.rs`. Test: `tests/integration.rs`.

**Interfaces:**
- Produces: `pub struct ProbeSettings` (`Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize`; private fields `sharpen_sigma_bits: u32`, `sharpen_mode`, `metric_mode`, `artifact_metric`, `metric_weights_bits: [u32; 4]`, `selection_policy`, `evaluation_color_space`) with `pub(crate) fn from_params(params: &AutoSharpParams) -> Self`, `pub(crate) fn needs_probe_breakdown(&self) -> bool`, `fn metric_weights(&self) -> MetricWeights`. Re-exported from `lib.rs`.
- Produces: `pub(crate) fn SelectionPolicy::needs_probe_breakdown(self) -> bool` in `types.rs`. Delete `AutoSharpParams::needs_probe_breakdown`; its callers use the settings.
- Changes (private): `ProbeContext { base, base_luminance, detail, baseline_artifact_ratio, settings: ProbeSettings }`; `fn probe_strengths(strengths, base, base_luminance, detail, baseline_artifact_ratio, settings: &ProbeSettings) -> Result<Vec<ProbeSample>, CoreError>` (drop its `too_many_arguments` allow); `fn run_two_pass_probing(coarse_count, coarse_min, coarse_max, dense_count, window_margin, p0, base, base_luminance, baseline_artifact_ratio, detail, settings: &ProbeSettings)`.

- [ ] **Step 1: Record the serial-path baseline before touching core**

Save the Appendix harness as `/tmp/opencode/zz_diff.rs`. Run the `base` half of the Appendix procedure (worktree at `8dad1da`) to produce `/tmp/opencode/zz_base.txt`.
Expected: `wc -l /tmp/opencode/zz_base.txt` prints 3456.

- [ ] **Step 2: Write the failing test**

In `integration.rs`, add `stripe_image` to the helpers, a new "Probe-worker seam" section after the LOO tests, and `process_from_prepared`, `EvaluationColorSpace`, `SelectionPolicy` to the imports.

```rust
/// Dark/light stripes 8 px wide: sharpening overshoot clips the red channel
/// while luminance stays in range, so the evaluation color spaces disagree.
fn stripe_image(w: u32, h: u32) -> LinearRgbImage {
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    for _ in 0..h {
        for x in 0..w {
            let v = if (x / 8) % 2 == 0 { 0.92 } else { 0.03 };
            data.extend_from_slice(&[v, v * 0.5, v * 0.2]);
        }
    }
    LinearRgbImage::new(w, h, data).unwrap()
}

#[test]
fn worker_probes_measure_what_the_pipeline_measures() {
    let src = stripe_image(128, 96);
    let strengths = [0.5f32, 1.0, 2.0, 3.0, 4.0];
    let photo = AutoSharpParams::photo(32, 24);
    let variants = [
        ("lightness", photo.clone()),
        ("rgb", AutoSharpParams { sharpen_mode: SharpenMode::Rgb, ..photo.clone() }),
        ("hybrid", AutoSharpParams { selection_policy: SelectionPolicy::Hybrid, ..photo.clone() }),
    ];
    for (label, variant) in variants {
        for cs in [None, Some(EvaluationColorSpace::Rgb), Some(EvaluationColorSpace::LumaOnly), Some(EvaluationColorSpace::LabApprox)] {
            let params = AutoSharpParams {
                evaluation_color_space: cs,
                probe_strengths: ProbeConfig::Explicit(strengths.to_vec()),
                ..variant.clone()
            };
            let prepared = prepare_base(&src, &params, &|_| {}).unwrap();
            let in_process = process_from_prepared(&prepared, &params, &|_| {}).unwrap().diagnostics.probe_samples;
            let worker = prepared.run_probes(&strengths, &params).unwrap();
            assert_eq!(worker.len(), in_process.len(), "{label}, evaluation_color_space = {cs:?}");
            for (w, p) in worker.iter().zip(&in_process) {
                assert_eq!(
                    (w.strength, w.artifact_ratio.to_bits(), w.metric_value.to_bits()),
                    (p.strength, p.artifact_ratio.to_bits(), p.metric_value.to_bits()),
                    "{label}, evaluation_color_space = {cs:?}"
                );
            }
        }
    }
}
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p r3sizer-core --test integration worker_probes_measure`
Expected: FAIL for `LumaOnly` and `LabApprox` only. At strength 0.5 the worker sample is near `0.507` and the in-process sample near `0.471`; `Rgb` and `None` pass.

- [ ] **Step 4: Add `ProbeSettings` and thread it through the measurement path**

Place `ProbeSettings` above `BaseParamsKey`. `from_params` fills the bit fields with `f32::to_bits`. In `probe_one_reuse`:

```rust
    let p_total = match ctx.settings.evaluation_color_space {
        Some(cs) => crate::chroma_guard::evaluate_in_color_space(&scratch.rgb, cs),
        None => crate::metrics::compute_selection_metric(&scratch.rgb, ctx.settings.artifact_metric),
    };
```

Remove `eval_cs_fn` and `metric_override` from `run_probes_for_prepared` and build `let settings = ProbeSettings::from_params(params);`. Replace every `params.needs_probe_breakdown()` with `settings.needs_probe_breakdown()`. `run_probes_standalone` and `run_probes_from_detail` build a `ProbeSettings` from `params` and pass it on; nothing else in them changes.

- [ ] **Step 5: Run to verify it passes**

Run: `cargo test -p r3sizer-core --test integration worker_probes_measure` and `cargo test -p r3sizer-core --lib probes_carry_breakdown`
Expected: PASS.

- [ ] **Step 6: Pin what `ProbeSettings` covers**

In `pipeline.rs` `mod tests`:

```rust
    const PROBE_SETTINGS_FIELDS: [&str; 7] = [
        "sharpen_sigma", "sharpen_mode", "metric_mode", "artifact_metric", "metric_weights",
        "selection_policy", "evaluation_color_space",
    ];

    #[test]
    fn probe_settings_change_exactly_when_a_probe_setting_field_changes() {
        let base = AutoSharpParams::photo(16, 12);
        let settings = ProbeSettings::from_params(&base);
        let all: Vec<&str> = BASE_AFFECTING.iter().chain(PROBE_ONLY.iter()).chain(NEITHER.iter()).copied().collect();
        assert_eq!(all.len(), 21);
        for name in all {
            let mut changed = base.clone();
            toggle_param(name, &mut changed);
            assert_eq!(ProbeSettings::from_params(&changed) != settings, PROBE_SETTINGS_FIELDS.contains(&name), "`{name}`");
        }
    }
```

Run: `cargo test -p r3sizer-core --lib probe_settings_change`
Expected: PASS. Then temporarily set `selection_policy: SelectionPolicy::GamutOnly,` in `from_params`: it must FAIL naming `selection_policy`. Revert.

- [ ] **Step 7: Prove the serial path is unchanged**

Run the `head` half of the Appendix procedure (worktree at `HEAD`).
Expected: `diff` reports nothing and prints `identical`; both files have 3456 lines.

- [ ] **Step 8: Commit**

```bash
git add -A crates
git commit -m "fix: measure every probe through one ProbeSettings value" -m "Worker probes ignored evaluation_color_space, so the parallel path\nmeasured a different metric than the in-process path whenever the\noption was set. Both paths now build their probe context from the\nsame ProbeSettings, and nothing else a probe reads is reachable."
```

---

### Task 3: Validated, stamped worker input in core

**Files:** Modify `crates/r3sizer-core/src/pipeline.rs` (`PreparedBase` ~85, `prepare_base` ~330, `process_from_prepared_with_probes` ~388, `PreparedBase::compute_detail` and `run_probes` ~148-193), `lib.rs`, and one temporary line in `crates/r3sizer-wasm/src/lib.rs` (`process_from_probes` ~506). Test: `tests/integration.rs`.

**Interfaces:**
- Produces: `static NEXT_BASE_ID: AtomicU64` (starts at 1); `PreparedBase` gains private `base_id: u64`, assigned in `prepare_base` with `fetch_add(1, Ordering::Relaxed)`.
- Produces: `CoreError::ProbeProvenanceMismatch` in `lib.rs`, `#[error("probe batch was not measured on this prepared base with these params")]`.
- Produces (in `pipeline.rs`, re-exported from `lib.rs`):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeProvenance { /* private: base_id: u64, baseline_artifact_ratio_bits: u32, settings: ProbeSettings */ }
impl ProbeProvenance { pub(crate) fn baseline_artifact_ratio(&self) -> f32 }

#[derive(Debug, Clone, Copy)]
pub struct ProbeWorkerParts<'a> {
    pub base: &'a LinearRgbImage,
    pub base_luminance: &'a [f32],
    pub detail: &'a [f32],
    pub baseline_artifact_ratio: f32,
}

#[derive(Debug)]
pub struct ProbeWorkerInput<'a> { /* private */ }
impl<'a> ProbeWorkerInput<'a> {
    pub fn new(parts: ProbeWorkerParts<'a>, params: &AutoSharpParams, provenance: ProbeProvenance) -> Result<Self, CoreError>;
    pub fn provenance(&self) -> ProbeProvenance;
    pub fn run_probes(&self, strengths: &[f32]) -> Result<ProbeBatch, CoreError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeBatch { /* private: provenance: ProbeProvenance, samples: Vec<ProbeSample> */ }
impl ProbeBatch {
    pub fn new(provenance: ProbeProvenance, samples: Vec<ProbeSample>) -> Self;
    pub fn provenance(&self) -> ProbeProvenance;
    pub fn samples(&self) -> &[ProbeSample];
}

impl PreparedBase {
    pub fn probe_provenance(&self, params: &AutoSharpParams) -> ProbeProvenance;
    pub fn probe_input<'a>(&'a self, detail: &'a [f32], params: &AutoSharpParams) -> Result<ProbeWorkerInput<'a>, CoreError>;
    pub fn run_probes(&self, strengths: &[f32], params: &AutoSharpParams) -> Result<ProbeBatch, CoreError>;
}
```

- Changes: `pub fn process_from_prepared_with_probes(prepared: &PreparedBase, params: &AutoSharpParams, batch: ProbeBatch, probing_us: u64, pass_diagnostics: Option<ProbePassDiagnostics>, detail: Option<Vec<f32>>, on_stage: &dyn Fn(&str)) -> Result<ProcessOutput, CoreError>`. Its first statement is the stamp check.
- `ProbeWorkerInput::run_probes` calls `probe_strengths(strengths, self.base, Some(self.base_luminance), self.detail, self.baseline_artifact_ratio, &self.settings)`.
- `PreparedBase::compute_detail` keeps its signature and calls the private `compute_detail` directly.

- [ ] **Step 1: Write the failing stamp tests**

In `integration.rs`, add `resolve_initial_strengths`, `CoreError`, `ProbeBatch`, `ProbeWorkerInput`, `ProbeWorkerParts` to the imports. `solve_with_probes` wraps its hand-made `Vec<ProbeSample>` in `ProbeBatch::new(prepared.probe_provenance(params), samples)`. The Task 2 test reads `prepared.run_probes(..).unwrap().samples()` instead of a `Vec`. Add:

```rust
fn seam_params() -> AutoSharpParams {
    AutoSharpParams { probe_strengths: ProbeConfig::Explicit(vec![0.25, 0.5, 1.0, 2.0, 3.0]), ..AutoSharpParams::photo(32, 24) }
}

/// `ProbeWorkerParts` with each buffer passed under its own name.
fn parts<'a>(base: &'a LinearRgbImage, luma: &'a [f32], detail: &'a [f32], baseline: f32) -> ProbeWorkerParts<'a> {
    ProbeWorkerParts { base, base_luminance: luma, detail, baseline_artifact_ratio: baseline }
}

#[test]
fn a_probe_batch_is_refused_by_another_instance_of_the_same_base() {
    let src = stripe_image(128, 96);
    let params = seam_params();
    let a = prepare_base(&src, &params, &|_| {}).unwrap();
    let b = prepare_base(&src, &params, &|_| {}).unwrap();
    assert_ne!(a.probe_provenance(&params), b.probe_provenance(&params));
    let batch = a.run_probes(&[0.5, 1.0, 2.0], &params).unwrap();
    let refused = process_from_prepared_with_probes(&b, &params, batch, 0, None, None, &|_| {});
    assert!(matches!(refused, Err(CoreError::ProbeProvenanceMismatch)));
}

#[test]
fn an_accepted_batch_gives_the_in_process_result() {
    let params = seam_params();
    let prepared = prepare_base(&stripe_image(128, 96), &params, &|_| {}).unwrap();
    let batch = prepared.run_probes(&[0.25, 0.5, 1.0, 2.0, 3.0], &params).unwrap();
    let via_workers = process_from_prepared_with_probes(&prepared, &params, batch, 0, None, None, &|_| {}).unwrap();
    let in_process = process_from_prepared(&prepared, &params, &|_| {}).unwrap();
    assert_eq!(via_workers.diagnostics.selected_strength.to_bits(), in_process.diagnostics.selected_strength.to_bits());
    let metrics = |d: &AutoSharpDiagnostics| d.probe_samples.iter().map(|s| (s.strength.to_bits(), s.metric_value.to_bits())).collect::<Vec<_>>();
    assert_eq!(metrics(&via_workers.diagnostics), metrics(&in_process.diagnostics));
}

#[test]
fn probing_no_strengths_gives_an_empty_stamped_batch() {
    let prepared = prepare_base(&stripe_image(128, 96), &seam_params(), &|_| {}).unwrap();
    let batch = prepared.run_probes(&[], &seam_params()).unwrap();
    assert!(batch.samples().is_empty());
    assert_eq!(batch.provenance(), prepared.probe_provenance(&seam_params()));
}
```

In `pipeline.rs` `mod tests`:

```rust
    #[test]
    fn the_finish_refuses_a_batch_whose_probe_settings_differ() {
        let prepared = prepared_photo();
        let base = AutoSharpParams::photo(16, 12);
        let batch = || prepared.run_probes(&[0.5, 1.0, 2.0], &base).unwrap();
        for name in BASE_AFFECTING.iter().chain(PROBE_ONLY.iter()).chain(NEITHER.iter()) {
            let mut changed = base.clone();
            toggle_param(name, &mut changed);
            let refused = matches!(
                process_from_prepared_with_probes(&prepared, &changed, batch(), 0, None, None, &|_| {}),
                Err(CoreError::ProbeProvenanceMismatch)
            );
            assert_eq!(refused, PROBE_SETTINGS_FIELDS.contains(name), "`{name}`");
        }
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p r3sizer-core --test integration a_probe_batch an_accepted_batch probing_no_strengths`
Expected: compile error, `failed to resolve: use of undeclared type ProbeBatch` (or `ProbeWorkerParts`).

- [ ] **Step 3: Implement the types, the instance id and the finish check**

Add the types in the Interfaces block. `ProbeWorkerInput::new` constructs the input for now without checking anything. `process_from_prepared_with_probes` starts with `if batch.provenance != prepared.probe_provenance(params) { return Err(CoreError::ProbeProvenanceMismatch); }`. `PreparedBase::run_probes` becomes `let detail = self.compute_detail(params)?; self.probe_input(&detail, params)?.run_probes(strengths)` and stops calling `run_probes_standalone`.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p r3sizer-core --test integration` and `cargo test -p r3sizer-core --lib the_finish_refuses`
Expected: PASS (the `loo_*` tests included).

- [ ] **Step 5: Keep the wasm crate compiling**

In `crates/r3sizer-wasm/src/lib.rs`, where `process_from_probes` passes `probe_samples` on, wrap them: `ProbeBatch::new(prepared.probe_provenance(&params), probe_samples)`. This stamps whatever arrives and validates nothing; Task 4 replaces it.
Run: `cargo check -p r3sizer-wasm --target wasm32-unknown-unknown`
Expected: Finished, no errors.

- [ ] **Step 6: Write the failing length and stamp tests**

In `integration.rs`:

```rust
#[test]
fn a_probe_input_rejects_every_wrong_length_buffer() {
    let src = stripe_image(128, 96);
    for mode in [SharpenMode::Lightness, SharpenMode::Rgb] {
        let params = AutoSharpParams { sharpen_mode: mode, ..seam_params() };
        let prepared = prepare_base(&src, &params, &|_| {}).unwrap();
        let base = LinearRgbImage::new(32, 24, prepared.base_pixels().to_vec()).unwrap();
        let luma = prepared.luminance().unwrap().to_vec();
        let detail = prepared.compute_detail(&params).unwrap();
        let stamp = prepared.probe_provenance(&params);
        let baseline = prepared.baseline_artifact_ratio();
        let n = 32 * 24;
        let (luma_len, detail_len) = (luma.len(), detail.len());
        assert_eq!(luma_len, n);
        assert_eq!(detail_len, if matches!(mode, SharpenMode::Lightness) { n } else { 3 * n });
        ProbeWorkerInput::new(parts(&base, &luma, &detail, baseline), &params, stamp).unwrap();

        let mut long_luma = luma.clone();
        long_luma.extend([0.5; 5]);
        for (bad, expected, got) in [(&luma[..n - 1], n, n - 1), (long_luma.as_slice(), n, n + 5)] {
            let result = ProbeWorkerInput::new(parts(&base, bad, &detail, baseline), &params, stamp);
            assert!(
                matches!(result, Err(CoreError::BufferLengthMismatch { expected_len, got_len }) if (expected_len, got_len) == (expected, got)),
                "{mode:?}: luminance of {} floats was accepted or misreported",
                bad.len()
            );
        }
        let mut long_detail = detail.clone();
        long_detail.extend([0.5; 7]);
        for (bad, expected, got) in [(&detail[..detail_len - 1], detail_len, detail_len - 1), (long_detail.as_slice(), detail_len, detail_len + 7)] {
            let result = ProbeWorkerInput::new(parts(&base, &luma, bad, baseline), &params, stamp);
            assert!(
                matches!(result, Err(CoreError::BufferLengthMismatch { expected_len, got_len }) if (expected_len, got_len) == (expected, got)),
                "{mode:?}: detail of {} floats was accepted or misreported",
                bad.len()
            );
        }
    }
}

#[test]
fn a_probe_input_rejects_a_stamp_minted_for_other_params_or_baseline() {
    let params = seam_params();
    let prepared = prepare_base(&stripe_image(128, 96), &params, &|_| {}).unwrap();
    let base = LinearRgbImage::new(32, 24, prepared.base_pixels().to_vec()).unwrap();
    let luma = prepared.luminance().unwrap();
    let detail = prepared.compute_detail(&params).unwrap();
    let stamp = prepared.probe_provenance(&params);
    let baseline = prepared.baseline_artifact_ratio();
    let other = AutoSharpParams { sharpen_sigma: 1.5, ..params.clone() };
    let wrong_params = ProbeWorkerInput::new(parts(&base, luma, &detail, baseline), &other, stamp);
    assert!(matches!(wrong_params, Err(CoreError::ProbeProvenanceMismatch)));
    let wrong_baseline = ProbeWorkerInput::new(parts(&base, luma, &detail, baseline + 0.125), &params, stamp);
    assert!(matches!(wrong_baseline, Err(CoreError::ProbeProvenanceMismatch)));
}
```

- [ ] **Step 7: Run to verify they fail**

Run: `cargo test -p r3sizer-core --test integration a_probe_input`
Expected: FAIL. The length test panics in `sharpen.rs` (`detail buffer too short` or `out buffer too short`) for Lightness mode; the stamp test's first assertion sees `Ok(..)`.

- [ ] **Step 8: Validate in `ProbeWorkerInput::new`**

In this order: `base_luminance.len() != W × H`; `detail.len() != W × H` (Lightness) or `W × H × 3` (Rgb); `provenance.settings != ProbeSettings::from_params(params)`; `provenance.baseline_artifact_ratio_bits != parts.baseline_artifact_ratio.to_bits()`. The first two return `BufferLengthMismatch`, the last two `ProbeProvenanceMismatch`. `PreparedBase::probe_input` calls `new` with the base's own buffers and a freshly minted stamp, so the same code validates it.

- [ ] **Step 9: Run to verify they pass**

Run: `cargo test -p r3sizer-core` and the Appendix `head` half.
Expected: all PASS; harness `identical`.

- [ ] **Step 10: Commit**

```bash
git add -A crates
git commit -m "fix: validate probe-worker buffers and stamp probe batches" -m "A wrong-length detail returned plausible but wrong samples in\nrelease builds, and process_from_prepared_with_probes fitted any\nsamples it was handed. Buffers now enter through one constructor that\nchecks every length, every batch carries the base instance, baseline\nand probe settings it was measured with, and the finish refuses any\nother stamp."
```

---

### Task 4: Wasm and web carry the stamp

**Files:** Create `crates/r3sizer-wasm/tests/probe_seam.rs`. Modify `crates/r3sizer-wasm/src/lib.rs` (`CACHED_DETAIL` and `DetailKey` ~30-40, `get_base_data` ~352, `probe_batch` ~399, `compute_probe_detail` ~432, `probe_batch_with_detail` ~465, `process_from_probes` ~500). Modify `web/src/shared/api/processing/probe-worker.ts`, `probe-pool.ts`, `wasm.ts`, `wasm-worker.ts`. Create `probe-pool.test.ts`, `probe-worker.test.ts` beside them.

**Interfaces:**
- Consumes: Task 3's `ProbeBatch`, `ProbeProvenance`, `ProbeWorkerInput`, `ProbeWorkerParts`.
- Produces (wasm; replaces `get_base_data`, `compute_probe_detail`, `probe_batch` and `DetailKey`):

```rust
#[wasm_bindgen]
pub fn get_probe_bundle(params_json: &str) -> Result<JsValue, JsValue>;
// { basePixels, luminance, width, height, baseline, effectiveP0, detail, provenanceJson }; Err when no base is cached

#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn probe_batch_with_detail(base_pixels: &[f32], width: u32, height: u32, luminance: &[f32], detail: &[f32], strengths_json: &str, params_json: &str, baseline: f32, provenance_json: &str) -> Result<String, JsValue>; // batch JSON

#[wasm_bindgen]
pub fn process_from_probes(params_json: &str, batch_json: &str, probing_us: u32, pass_diagnostics_json: &str) -> Result<JsValue, JsValue>;
```

  `CACHED_DETAIL` becomes `Option<(ProbeProvenance, Vec<f32>)>`. Parse errors for the new arguments start with `invalid provenance JSON:` and `invalid probe batch JSON:`.
- Produces (web):

```ts
// probe-worker.ts
export interface BaseData { basePixels: Float32Array; luminance: Float32Array; width: number; height: number; baseline: number; effectiveP0: number; detail: Float32Array; paramsJson: string; provenanceJson: string; }
export function runProbeBatch(base: BaseData, strengths: number[]): string; // calls probe_batch_with_detail in the binding's argument order
// ProbeWorkerRequest: { type: "init" | "set_base" | "probe" | "clear_base"; module?; id?; baseData?: BaseData; strengths?: number[] }

// probe-pool.ts
export type { BaseData } from "./probe-worker";
export interface ProbePoolResult { batchJson: string; probingMs: number }
export function mergeProbeBatches(batchJsons: string[]): string; // throws /no probe batches to merge/ on an empty round, /disagree on provenance/ on mixed stamps
export function runProbesParallel(strengths: number[], opts?: ProbeRunOpts): Promise<ProbePoolResult>;

// wasm-worker.ts
export interface ProbeBundle { basePixels: Float32Array; luminance: Float32Array; width: number; height: number; baseline: number; effectiveP0: number; detail: Float32Array; provenanceJson: string; }
// WorkerRequest.type gains "get_probe_bundle" and loses "get_base_data" | "compute_detail"; `probesJson` becomes `batchJson`
// WorkerResponse.type gains "probe_bundle" and loses "base_data" | "detail"; field `bundle?: ProbeBundle`
```

- [ ] **Step 1: Write the failing wasm tests**

Create `crates/r3sizer-wasm/tests/probe_seam.rs`, starting with `#![cfg(target_arch = "wasm32")]`, `use wasm_bindgen::{JsCast, JsValue};`, `use wasm_bindgen_test::wasm_bindgen_test;`, and these helpers:

- `fn stripe_rgba(w: u32, h: u32) -> Vec<u8>`: 8 px dark/light stripes, each pixel `[v, v / 2, v / 5, 255]` with `v` = 235 or 8.
- `fn params_json(sigma: f32) -> String`: `serde_json::to_string` of `AutoSharpParams::photo(32, 24)` with `sharpen_sigma = sigma`.
- `struct Bundle { base_pixels, luminance, detail: Vec<f32>, width, height: u32, baseline, effective_p0: f32, provenance_json: String }`.
- `fn bundle(params_json: &str) -> Bundle`: `get_probe_bundle(..).unwrap()`, reading `basePixels`, `luminance` and `detail` as `Float32Array`, the four numbers, and `provenanceJson`.
- `fn probe(b: &Bundle, params_json: &str, strengths: &[f32], baseline: f32, provenance: &str) -> Result<String, JsValue>`: `probe_batch_with_detail` with `b.base_pixels, b.width, b.height, &b.luminance, &b.detail`, the JSON of `strengths`, `params_json`, `baseline`, `provenance`.
- `fn merge(batches: &[String]) -> String`: assert every `provenance` is equal, concatenate `samples`, sort by `strength`, return JSON.
- `fn probe_metrics(result: &JsValue) -> Vec<(u32, u32)>`: `(strength bits, metric_value bits)` of `result.diagnostics.probe_samples`, read with `js_sys::Reflect`.

```rust
#[wasm_bindgen_test]
fn probes_through_the_wasm_boundary_match_the_in_process_pipeline() {
    r3sizer_wasm::clear_cache();
    let rgba = stripe_rgba(128, 96);
    let params = params_json(1.0);
    r3sizer_wasm::prepare_base(&rgba, 128, 96, &params).unwrap();
    let b = bundle(&params);
    assert_eq!((b.width, b.height, b.luminance.len(), b.detail.len()), (32, 24, 32 * 24, 32 * 24));

    // Coarse round, dense window, dense round, merge: the web client's sequence.
    let coarse_strengths: Vec<f32> = serde_json::from_str(&r3sizer_wasm::resolve_initial_strengths(&params).unwrap()).unwrap();
    let coarse = probe(&b, &params, &coarse_strengths, b.baseline, &b.provenance_json).unwrap();
    let coarse_samples = serde_json::to_string(&serde_json::from_str::<serde_json::Value>(&coarse).unwrap()["samples"]).unwrap();
    let dense = r3sizer_wasm::resolve_dense_strengths(&coarse_samples, &params, b.effective_p0).unwrap();
    let dense_strengths: Vec<f32> = serde_wasm_bindgen::from_value(js_sys::Reflect::get(&dense, &"strengths".into()).unwrap()).unwrap();
    let dense_batch = probe(&b, &params, &dense_strengths, b.baseline, &b.provenance_json).unwrap();
    let via_workers = r3sizer_wasm::process_from_probes(&params, &merge(&[coarse, dense_batch]), 0, "").unwrap();

    // The web path probes every coarse strength; the in-process path may stop
    // early.  Every in-process sample must appear, bit for bit, in the batch.
    let in_process = r3sizer_wasm::process_image(&rgba, 128, 96, &params).unwrap();
    let (via, inproc) = (probe_metrics(&via_workers), probe_metrics(&in_process));
    assert!(via.len() >= inproc.len());
    for sample in &inproc {
        assert!(via.contains(sample), "in-process sample {sample:?} differs or is missing through the worker path");
    }
}

#[wasm_bindgen_test]
fn process_from_probes_refuses_a_stale_batch() {
    r3sizer_wasm::clear_cache();
    let rgba = stripe_rgba(128, 96);
    let params = params_json(1.0);
    r3sizer_wasm::prepare_base(&rgba, 128, 96, &params).unwrap();
    let b = bundle(&params);
    let batch = probe(&b, &params, &[0.5, 1.0], b.baseline, &b.provenance_json).unwrap();
    r3sizer_wasm::process_from_probes(&params, &batch, 0, "").unwrap();

    // Another sigma keeps the cached base (fast path) but changes the detail.
    let other_sigma = params_json(1.5);
    r3sizer_wasm::prepare_base(&rgba, 128, 96, &other_sigma).unwrap();
    let err = r3sizer_wasm::process_from_probes(&other_sigma, &batch, 0, "").unwrap_err();
    assert!(err.as_string().unwrap().contains("not measured on this prepared base"));

    // An identical re-preparation is a new base instance.
    r3sizer_wasm::clear_cache();
    r3sizer_wasm::prepare_base(&rgba, 128, 96, &params).unwrap();
    let err = r3sizer_wasm::process_from_probes(&params, &batch, 0, "").unwrap_err();
    assert!(err.as_string().unwrap().contains("not measured on this prepared base"));
}

#[wasm_bindgen_test]
fn a_worker_rejects_wrong_buffers_and_foreign_stamps() {
    r3sizer_wasm::clear_cache();
    let rgba = stripe_rgba(128, 96);
    let params = params_json(1.0);
    r3sizer_wasm::prepare_base(&rgba, 128, 96, &params).unwrap();
    let b = bundle(&params);
    let foreign = bundle(&params_json(1.5)).provenance_json;
    let short = &b.detail[..b.detail.len() - 1];

    let wrong_detail = r3sizer_wasm::probe_batch_with_detail(&b.base_pixels, b.width, b.height, &b.luminance, short, "[0.5]", &params, b.baseline, &b.provenance_json).unwrap_err();
    assert!(wrong_detail.as_string().unwrap().contains("buffer length mismatch: expected 768"));
    for err in [
        probe(&b, &params, &[0.5], b.baseline, &foreign).unwrap_err(),
        probe(&b, &params, &[0.5], b.baseline + 0.125, &b.provenance_json).unwrap_err(),
    ] {
        assert!(err.as_string().unwrap().contains("not measured on this prepared base"));
    }
    let garbage = probe(&b, &params, &[0.5], b.baseline, "not json").unwrap_err();
    assert!(garbage.as_string().unwrap().starts_with("invalid provenance JSON:"));
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `wasm-pack test --node crates/r3sizer-wasm`
Expected: compile error, `cannot find function get_probe_bundle in crate r3sizer_wasm`.

- [ ] **Step 3: Implement the wasm exports**

Per the Interfaces block. `get_probe_bundle` mints the provenance, computes the detail with `prepared.compute_detail(&params)`, builds the object, and stores `Some((provenance, detail))` in `CACHED_DETAIL`. `probe_batch_with_detail` builds `LinearRgbImage::new(width, height, base_pixels.to_vec())` and a `ProbeWorkerParts` by field name, then calls `ProbeWorkerInput::new(..)` and `run_probes`. `process_from_probes` deserializes a `ProbeBatch` from `batch_json`, takes `CACHED_DETAIL` only when its provenance equals `prepared.probe_provenance(&params)`, and deletes the temporary stamping line from Task 3. Update the doc comments on the three exports.

- [ ] **Step 4: Run to verify they pass**

Run: `wasm-pack test --node crates/r3sizer-wasm`
Expected: 4 tests pass (`metadata_response_uses_typed_bytes_and_nullable_fields` and the three new ones).

- [ ] **Step 5: Write the failing web tests**

`probe-pool.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { mergeProbeBatches } from "./probe-pool";

const stamp = { base_id: 3, baseline_artifact_ratio_bits: 1050253722, settings: { sharpen_sigma_bits: 1065353216 } };
const sample = (strength: number) => ({ strength, artifact_ratio: 0.1, metric_value: 0.1 });
const batch = (provenance: unknown, samples: unknown[]) => JSON.stringify({ provenance, samples });

describe("mergeProbeBatches", () => {
  it("keeps one stamp and sorts and deduplicates the samples", () => {
    const merged = JSON.parse(mergeProbeBatches([batch(stamp, [sample(1), sample(0.5)]), batch(stamp, [sample(1 + 1e-6), sample(2)])]));
    expect(merged.provenance).toEqual(stamp);
    expect(merged.samples.map((s: { strength: number }) => s.strength)).toEqual([0.5, 1, 2]);
  });

  it("refuses batches that disagree on the stamp", () => {
    expect(() => mergeProbeBatches([batch(stamp, [sample(1)]), batch({ ...stamp, base_id: 4 }, [sample(2)])])).toThrow(/disagree on provenance/);
  });

  it("refuses an empty round", () => {
    expect(() => mergeProbeBatches([])).toThrow(/no probe batches to merge/);
  });
});
```

`probe-worker.test.ts`:

```ts
import { beforeEach, expect, it, vi } from "vitest";

vi.mock("./wasm-pkg/r3sizer_wasm", () => ({ initSync: vi.fn(), probe_batch_with_detail: vi.fn() }));

import { probe_batch_with_detail } from "./wasm-pkg/r3sizer_wasm";
import { type BaseData, runProbeBatch } from "./probe-worker";

const base: BaseData = {
  basePixels: new Float32Array(3), luminance: new Float32Array(1), width: 1, height: 1, baseline: 0.25,
  effectiveP0: 0.5, detail: new Float32Array(1), paramsJson: "{\"target_width\":1}", provenanceJson: "{\"base_id\":7}",
};

beforeEach(() => vi.mocked(probe_batch_with_detail).mockReset());

// In Lightness mode `luminance` and `detail` are both W×H floats, so a swap at
// this call site would compile and run. The binding's order is the contract.
it("passes the cached base to wasm in the binding's argument order", () => {
  vi.mocked(probe_batch_with_detail).mockReturnValueOnce("{\"provenance\":{},\"samples\":[]}");
  expect(runProbeBatch(base, [0.5, 1])).toBe("{\"provenance\":{},\"samples\":[]}");
  expect(vi.mocked(probe_batch_with_detail).mock.calls[0]).toEqual([
    base.basePixels, 1, 1, base.luminance, base.detail, "[0.5,1]", base.paramsJson, 0.25, base.provenanceJson,
  ]);
});
```

- [ ] **Step 6: Run to verify they fail**

Run: `cd web && npx vitest run src/shared/api/processing/probe-pool.test.ts src/shared/api/processing/probe-worker.test.ts`
Expected: FAIL. `mergeProbeBatches` is not exported; `runProbeBatch` is not exported.

- [ ] **Step 7: Implement the web glue**

Per the Interfaces block, and:

- `probe-worker.ts`: `set_base` stores `msg.baseData` as the whole cached record; `probe` calls `runProbeBatch(cachedBase, strengths)`.
- `probe-pool.ts`: `distributeBaseData` posts `{ type: "set_base", id, baseData }`; a `probe` message carries strengths only; `runProbesParallel` returns `mergeProbeBatches(results)` as `batchJson`, plus `probingMs`.
- `wasm.ts` `runParallelPipeline`: `const bundle = await callWorker<ProbeBundle>({ type: "get_probe_bundle", paramsJson })`; `const baseData: BaseData = { ...bundle, paramsJson }`; `runProbesParallel(strengths, opts)`; coarse samples for the dense window come from `JSON.parse(coarse.batchJson).samples`; the final message is `{ type: "process_from_probes", paramsJson, batchJson: mergeProbeBatches([coarse.batchJson, dense.batchJson]), probingUs, passDiagnosticsJson }`. Delete `getBaseData` and the inline sort and dedup.
- `wasm-worker.ts`: one `get_probe_bundle` handler replaces the `get_base_data` and `compute_detail` handlers; `process_from_probes` sends `batchJson`.

- [ ] **Step 8: Rebuild the package and run the web gates**

Run: `cd web && npm run build:wasm && npm run lint && npm run fsd && npx tsc -b && npm test`
Expected: all clean; `npm test` reports 83 passed (79 + 4 new).

- [ ] **Step 9: Commit**

```bash
git add -A crates/r3sizer-wasm web/src
git commit -m "fix: carry the probe stamp through wasm and the web pool" -m "The web pool paired whatever params a message carried with whatever\nbuffers a worker had cached, and nothing tied a batch to the base it\nran on. The main worker now mints base, detail and stamp together,\neach worker probes with the record it was given, and a merge refuses\nbatches that disagree on the stamp."
```

---

### Task 5: Remove the superseded entry points

**Files:** Modify `crates/r3sizer-core/src/pipeline.rs` (`run_probes_standalone` ~482, `compute_probe_detail` ~517, `run_probes_from_detail` ~549, doc comments on `PreparedBase::compute_detail` and `run_probes`, `mod tests`) and `lib.rs`.

**Interfaces:**
- Removes: `pub fn run_probes_standalone`, `pub fn run_probes_from_detail`, `pub fn compute_probe_detail` and their re-exports.
- `PreparedBase::compute_detail` keeps its signature; its body becomes `let kernel = make_kernel(params.sharpen_sigma)?; Ok(compute_detail(&self.base, self.base_luminance.as_deref(), params.sharpen_mode, &kernel))`.

- [ ] **Step 1: Confirm nothing outside tests uses them**

Run: `grep -rn "run_probes_standalone\|run_probes_from_detail\|compute_probe_detail" crates web/src --include=*.rs --include=*.ts | grep -v wasm-pkg`
Expected: only their definitions in `pipeline.rs`, the `lib.rs` re-export line, and `probes_carry_breakdown_only_under_composite_policy`.

- [ ] **Step 2: Rewrite the one test that used `run_probes_standalone`**

```rust
    #[test]
    fn probes_carry_breakdown_only_under_composite_policy() {
        use crate::SelectionPolicy;

        let src = gradient(96, 72);
        let strengths = [0.5f32, 1.0, 2.0, 3.0];
        let mut params = AutoSharpParams::photo(24, 18);
        let prepared = prepare_base(&src, &params, &|_| {}).unwrap();

        params.selection_policy = SelectionPolicy::GamutOnly;
        let gamut = prepared.run_probes(&strengths, &params).unwrap();
        assert!(gamut.samples().iter().all(|s| s.breakdown.is_none()));

        params.selection_policy = SelectionPolicy::Hybrid;
        let hybrid = prepared.run_probes(&strengths, &params).unwrap();
        assert!(hybrid.samples().iter().all(|s| s.breakdown.is_some()));
    }
```

- [ ] **Step 3: Delete the three functions and their re-exports**

Also repoint the doc comments on `PreparedBase::compute_detail` and `run_probes` from the removed functions to [`ProbeWorkerInput`] and [`PreparedBase::probe_input`].

- [ ] **Step 4: Verify**

Run: `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`, and the Appendix `head` half.
Expected: clean; harness `identical`.

- [ ] **Step 5: Commit**

```bash
git add -A crates
git commit -m "refactor: remove the unvalidated probe-worker entry points" -m "run_probes_standalone, run_probes_from_detail and compute_probe_detail\ntook raw positional slices and returned unstamped samples.\nProbeWorkerInput::new is now the only raw entry, and it validates\nand stamps everything it accepts."
```

---

### Task 6: Docs and changelog

**Files:** Modify `CHANGELOG.md` (`[Unreleased]`), `CLAUDE.md` (lines 57, 65, 67, 94, 95), `docs/architecture.md` (lines 183-235), `docs/future_work.md` (line 36).

- [ ] **Step 1: CHANGELOG**

Under `### Fixed`:

```markdown
- Probe workers reject buffers of the wrong length with
  `CoreError::BufferLengthMismatch`. In release builds a too-short `detail`
  used to return plausible but wrong samples, and with it a wrong s*.
- Worker probes honor `evaluation_color_space`, matching the in-process
  pipeline. They used to measure as if the option were off.
```

Under `### Changed`:

```markdown
- `process_from_prepared_with_probes` takes a `ProbeBatch` and returns
  `CoreError::ProbeProvenanceMismatch` unless the batch was measured on this
  `PreparedBase` instance, with this baseline and these probe settings.
  `PreparedBase::run_probes` returns a `ProbeBatch`.
  `run_probes_standalone`, `run_probes_from_detail` and `compute_probe_detail`
  are replaced by `ProbeWorkerInput`, the one raw entry, which validates every
  buffer length.
- WASM: `get_probe_bundle` replaces `get_base_data` and
  `compute_probe_detail`; `probe_batch_with_detail` takes a provenance and
  returns a batch; `process_from_probes` takes a batch. `probe_batch` is
  removed.
```

- [ ] **Step 2: CLAUDE.md**

- Line 57: replace `` `compute_probe_detail` / `run_probes_from_detail` `` with `` `ProbeWorkerInput` ``.
- Line 65: in the exports list, `get_probe_bundle` (base, detail and provenance minted in one call), `probe_batch_with_detail` (run probes using precomputed detail, returning a stamped batch), `process_from_probes` (finish pipeline with a stamped batch, refusing any other). Drop `get_base_data` and `compute_probe_detail`.
- Line 67: probe workers "receive the base, its detail signal, and the params and provenance it was minted for in one `set_base` record".
- Line 95: `CACHED_DETAIL` is "keyed by the probe provenance (base instance, baseline, probe settings)".
- Add a design-decision bullet after line 96: **Probe batches are stamped and checked**: `ProbeWorkerInput::new` is the only raw entry and validates every buffer length exactly; each batch carries the `PreparedBase` instance, baseline and probe settings it was measured with; `process_from_prepared_with_probes` refuses any other stamp. An identical re-preparation is a new instance, so the web client falls back to the in-process path.

- [ ] **Step 3: docs/architecture.md**

In the sequence diagram: the `par` block (lines 187-192) becomes one step, `UI->>MW: get_probe_bundle(params)`, answered by "Base pixels, the detail signal and the provenance stamp"; line 195 becomes `set_base once: bundle, params and stamp, cached in every worker`; lines 201 and 207 return "Stamped coarse samples" and "Stamped dense samples"; line 208 becomes "Merge under one stamp, sort by strength, deduplicate"; line 210 becomes `process_from_probes(merged batch)`; line 212 gains "refusing a foreign stamp,". Change "Three design points" to "Four" and add, after the blur paragraph:

```markdown
**A batch carries its provenance.** The stamp names the `PreparedBase`
instance, its baseline and the settings the probes read. A worker cannot pair
one base's buffers with another's params, because params and stamp travel with
the buffers in one `set_base` record. `process_from_probes` refuses any batch
whose stamp is not the one it would mint for the base and params it holds.
```

- [ ] **Step 4: docs/future_work.md line 36**

Replace `` `compute_probe_detail` / `probe_batch_with_detail` `` with `` `get_probe_bundle` / `probe_batch_with_detail` ``.

- [ ] **Step 5: Run every gate**

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo test --workspace
cargo test -p r3sizer-core --features typegen export_typescript_bindings -- --nocapture
git diff --exit-code -- web/src/shared/lib/types/generated.ts
cargo check -p r3sizer-wasm --target wasm32-unknown-unknown
wasm-pack test --node crates/r3sizer-wasm
cd web && npm run lint && npm run fsd && npx tsc -b && npm test
```

Expected: all clean. `cargo test --workspace` reports 585 passed (574 on `main`: Task 1 adds 3 unit tests, Task 2 adds 2, Task 3 adds 6). The 3 wasm tests run under `wasm-pack test`; the web suite reports 83 passed.

- [ ] **Step 6: Commit**

```bash
git add -A CHANGELOG.md CLAUDE.md docs
git commit -m "docs: describe the stamped probe-worker seam"
```

---

## Appendix: differential harness

`/tmp/opencode` is cleared between days, so the harness lives here. It is never committed. Save it as `/tmp/opencode/zz_diff.rs`:

```rust
//! Throwaway differential harness for ticket #42.  Never committed.
use r3sizer_core::*;

fn textured(w: u32, h: u32, seed: u32) -> LinearRgbImage {
    let mut s = seed.wrapping_mul(2654435761).wrapping_add(1);
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            let n = (s >> 8) as f32 / 16777216.0;
            let edge = if (x as i32 - y as i32).abs() < 3 { 0.9f32 } else { 0.2 };
            let v = (edge * 0.7 + n * 0.3).clamp(0.0, 1.0);
            data.extend_from_slice(&[v, v * 0.9, v * 0.8]);
        }
    }
    LinearRgbImage::new(w, h, data).unwrap()
}

fn mix(h: u64, words: impl IntoIterator<Item = u32>) -> u64 {
    words.into_iter().fold(h, |h, w| (h ^ w as u64).wrapping_mul(0x100000001b3))
}

#[test]
fn zz_two_phase() {
    println!();
    for seed in 1..=8u32 {
        let src = textured(96, 72, seed);
        for mode in [None, Some(PipelineMode::Fast), Some(PipelineMode::Quality)] {
            for metric_mode in [MetricMode::RelativeToBase, MetricMode::AbsoluteTotal] {
                for p0 in [0.002f32, 0.006, 0.02] {
                    for fit in [FitStrategy::Cubic, FitStrategy::DirectSearch] {
                        for sharpen_mode in [SharpenMode::Lightness, SharpenMode::Rgb] {
                            for eval_cs in [None, Some(EvaluationColorSpace::LumaOnly), Some(EvaluationColorSpace::LabApprox)] {
                                for policy in [SelectionPolicy::GamutOnly, SelectionPolicy::Hybrid] {
                                    let params = AutoSharpParams {
                                        metric_mode, target_artifact_ratio: p0, fit_strategy: fit, sharpen_mode,
                                        evaluation_color_space: eval_cs, selection_policy: policy, pipeline_mode: mode,
                                        ..AutoSharpParams::photo(24, 18)
                                    }.resolved();
                                    let out = process_auto_sharp_downscale(&src, &params).unwrap();
                                    let d = &out.diagnostics;
                                    let h = mix(0xcbf29ce484222325, out.image.pixels().iter().map(|v| v.to_bits()));
                                    let h = mix(h, d.probe_samples.iter().flat_map(|s| {
                                        [s.strength.to_bits(), s.artifact_ratio.to_bits(), s.metric_value.to_bits()]
                                    }));
                                    println!("P/{seed}/{mode:?}/{metric_mode:?}/{p0}/{fit:?}/{sharpen_mode:?}/{eval_cs:?}/{policy:?}|{}|{:?}|{:?}|{:?}|{}|{h:016x}",
                                        d.selected_strength.to_bits(), d.selection_mode, d.fallback_reason, d.crossing_status, d.budget_reachable);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
```

Procedure. `base` means a worktree at `8dad1da` (`main`); `head` means one at `HEAD`.

```sh
cd /home/alvy/projects/r3sizer
for w in base head; do git worktree add --detach /tmp/opencode/r3sizer-42-$w $( [ $w = base ] && echo 8dad1da || echo HEAD ); done
for w in base head; do cp /tmp/opencode/zz_diff.rs /tmp/opencode/r3sizer-42-$w/crates/r3sizer-core/tests/zz_diff.rs; done
for w in base head; do (cd /tmp/opencode/r3sizer-42-$w && CARGO_TARGET_DIR=/home/alvy/projects/r3sizer/target cargo test -p r3sizer-core --test zz_diff -- --nocapture 2>/dev/null | grep '^P/' | sort > /tmp/opencode/zz_$w.txt); done
wc -l /tmp/opencode/zz_base.txt /tmp/opencode/zz_head.txt
diff /tmp/opencode/zz_base.txt /tmp/opencode/zz_head.txt && echo identical
for w in base head; do git worktree remove --force /tmp/opencode/r3sizer-42-$w; done
```
