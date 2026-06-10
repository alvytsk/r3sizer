# Striped Ingest + Processing Worker API Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Process 100MP+ images in the browser with a bounded WASM heap via streaming row-stripe ingest, plus a web processing facade with stage-level progress and cooperative cancellation.

**Architecture:** A new `r3sizer-core::ingest` module implements a streaming area-weighted pre-reduce (`StripedPreReducer`) that accepts sequential sRGB RGBA8 row stripes and accumulates directly into the ~2×-target linear intermediate image — the same intermediate the existing staged-shrink path produces. New WASM exports (`ingest_begin/stripe/end/abort`) feed it and store the result in the existing thread-local input cache, so the entire downstream protocol (prepare_base, probe pool, process_from_probes) works unchanged. On the web side, `wasm.ts`/`wasm-worker.ts`/`probe-pool.ts`/`probe-worker.ts` move into `web/src/processing/` behind a facade (`processingClient`) that selects monolithic vs. striped by a 24MP threshold, aggregates progress across stages, and supports cooperative cancellation.

**Tech Stack:** Rust (r3sizer-core, r3sizer-wasm/wasm-bindgen), ts-rs typegen, React 19 + Zustand + Vite, vitest (new), OffscreenCanvas/ImageBitmap.

**Spec:** `docs/superpowers/specs/2026-06-10-striped-ingest-worker-api-design.md`

**One deliberate adaptation from the spec's §7 sketch:** the facade splits `client.process(file, params)` into `client.decode(file)` (at upload time — the store needs source dimensions for aspect-ratio logic and a preview immediately) + `client.process(params)` (per run, reusing the cached decode). The `decode` progress stage therefore happens at upload, not inside the job; job stages are `ingest → prepare → probe → finalize` (striped) or `prepare → probe → finalize` (monolithic). Everything else (one facade, workers fully hidden, one active job, cancel/onProgress/promise) matches the spec.

---

## File Structure

**Rust — create:**
- `crates/r3sizer-core/src/ingest.rs` — `compute_intermediate_size`, `validate_striped_shrink`, `StripedPreReducer` (all new logic + unit tests)
- `crates/r3sizer-core/tests/striped_ingest.rs` — pipeline-parity integration test

**Rust — modify:**
- `crates/r3sizer-core/src/lib.rs` — new `CoreError` variants, `pub mod ingest`, exports
- `crates/r3sizer-core/src/resize.rs` — extract intermediate-size math to `ingest::compute_intermediate_size`; make `STAGED_SHRINK_THRESHOLD` and `fir_resize` `pub(crate)`
- `crates/r3sizer-core/src/color.rs` — add 256-entry `SRGB_U8_TO_LINEAR` LUT (moved from wasm)
- `crates/r3sizer-core/src/types.rs` — `IngestDiagnostics` + `AutoSharpDiagnostics.ingest` field
- `crates/r3sizer-core/src/pipeline.rs` — `ingest: None` in the diagnostics literal
- `crates/r3sizer-core/src/prelude.rs` — re-exports
- `crates/r3sizer-core/tests/typegen.rs` — export `IngestDiagnostics`
- `crates/r3sizer-wasm/src/convert.rs` — use the core LUT
- `crates/r3sizer-wasm/src/lib.rs` — ingest exports, cache wiring, constraint enforcement, diag injection

**Web — create:**
- `web/vitest.config.ts`, test files
- `web/src/processing/index.ts` — barrel (facade only)
- `web/src/processing/client.ts` — `ProcessingClient`, `JobImpl`, threshold constant
- `web/src/processing/ingest.ts` — decode, stripe planning/extraction, preview
- `web/src/processing/progress.ts` — stages, weights, `ProgressAggregator`
- `web/src/processing/errors.ts` — `CancelledError`, `ProcessingError`, `CancellationToken`
- `web/tools/generate-large-image.html` — 100MP test-image generator

**Web — move (git mv, then modify):**
- `web/src/wasm.ts` → `web/src/processing/wasm.ts`
- `web/src/wasm-worker.ts` → `web/src/processing/wasm-worker.ts`
- `web/src/probe-pool.ts` → `web/src/processing/probe-pool.ts`
- `web/src/probe-worker.ts` → `web/src/processing/probe-worker.ts`

**Web — modify:** `stores/processor-store.ts`, `App.tsx`, `components/ImageUpload.tsx`, `components/ImagePreview.tsx`, `components/ProcessingOverlay.tsx`, `components/diagnostics/SummaryTab.tsx`, `public/locales/en.json`, `public/locales/ru.json`, `package.json`, `types/generated.ts` (regenerated)

**Web — delete:** `web/src/lib/image-loader.ts`

All `cargo`/`git` commands run from the repo root; all `npm` commands run from `web/`.

---

### Task 1: `compute_intermediate_size` shared between staged shrink and ingest

The staged-shrink path in `resize.rs:18-82` computes the bilinear pre-reduce size inline. Extract that math into the new `ingest` module so the striped path produces the *identical* intermediate dimensions.

**Files:**
- Create: `crates/r3sizer-core/src/ingest.rs`
- Modify: `crates/r3sizer-core/src/lib.rs` (module decl)
- Modify: `crates/r3sizer-core/src/resize.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/r3sizer-core/src/ingest.rs`:

```rust
//! Streaming (striped) ingest for very large images.
//!
//! [`StripedPreReducer`] accepts sequential full-width row stripes of sRGB
//! RGBA8 and accumulates an area-weighted pre-reduce directly into the linear
//! intermediate image (~2x the final target) — the same intermediate the
//! staged-shrink path in [`crate::resize`] produces with a bilinear pass.
//! This bounds peak memory to one stripe plus the intermediate, instead of
//! the full source image.

use crate::types::ImageSize;

/// Intermediate (pre-reduce) size for a staged downscale: ~2x the target.
///
/// This is the exact same math the staged-shrink path in `resize.rs` uses,
/// so an image ingested in stripes lands on the identical intermediate
/// dimensions as one resized monolithically.
pub fn compute_intermediate_size(src: ImageSize, target: ImageSize) -> ImageSize {
    let shrink_x = src.width as f64 / target.width as f64;
    let shrink_y = src.height as f64 / target.height as f64;
    let max_shrink = shrink_x.max(shrink_y);
    let pre_factor = (max_shrink / 2.0).floor().max(1.0);
    ImageSize {
        width: ((src.width as f64 / pre_factor).round() as u32).max(target.width),
        height: ((src.height as f64 / pre_factor).round() as u32).max(target.height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intermediate_size_matches_staged_shrink_math() {
        // 6000x4000 -> 800x600: max_shrink = 7.5, pre_factor = floor(3.75) = 3
        let inter = compute_intermediate_size(
            ImageSize { width: 6000, height: 4000 },
            ImageSize { width: 800, height: 600 },
        );
        assert_eq!(inter, ImageSize { width: 2000, height: 1333 });
    }

    #[test]
    fn intermediate_size_never_smaller_than_target() {
        // Extremely wide: 40000x800 -> 1600x100. max_shrink = 25, pre = 12.
        // 800/12 = 66.7 -> clamped up to target height 100.
        let inter = compute_intermediate_size(
            ImageSize { width: 40000, height: 800 },
            ImageSize { width: 1600, height: 100 },
        );
        assert_eq!(inter, ImageSize { width: 3333, height: 100 });
    }
}
```

Add to `crates/r3sizer-core/src/lib.rs` next to the other module declarations (after `pub mod fit;`):

```rust
pub mod ingest;
```

- [ ] **Step 2: Run test to verify it passes (math is self-contained), then verify the refactor target still fails clippy if inconsistent**

Run: `cargo test -p r3sizer-core ingest:: -- --nocapture`
Expected: 2 tests PASS (the function is new code; the *refactor* below is what the existing resize tests guard).

- [ ] **Step 3: Refactor `resize.rs` to use the shared function**

In `crates/r3sizer-core/src/resize.rs`, change the threshold constant's visibility (the ingest validation in Task 2 reuses it):

```rust
pub(crate) const STAGED_SHRINK_THRESHOLD: f64 = 3.0;
```

Replace the inline pre-reduce size computation inside `downscale_with_info` (the `if max_shrink >= STAGED_SHRINK_THRESHOLD` branch currently computing `pre_factor`, `pre_w`, `pre_h`):

```rust
    if max_shrink >= STAGED_SHRINK_THRESHOLD {
        // Two-stage: fast bilinear pre-reduce to ~2× target, then Lanczos3.
        let inter = crate::ingest::compute_intermediate_size(src.size(), target);
        let pre = fir_resize(
            src,
            inter.width,
            inter.height,
            fir::ResizeAlg::Convolution(fir::FilterType::Bilinear),
        )?;
```

(The rest of the branch — the Lanczos3 pass on `pre` — is unchanged.)

Also make `fir_resize` crate-visible (Task 6's tolerance test needs the bilinear reference):

```rust
pub(crate) fn fir_resize(
```

- [ ] **Step 4: Run the full core test suite**

Run: `cargo test -p r3sizer-core`
Expected: all tests PASS (existing resize/staged-shrink tests prove the refactor preserved behavior).

- [ ] **Step 5: Lint and commit**

```bash
cargo clippy --workspace -- -D warnings
git add crates/r3sizer-core/src/ingest.rs crates/r3sizer-core/src/lib.rs crates/r3sizer-core/src/resize.rs
git commit -m "feat(core): extract shared intermediate-size computation into ingest module"
```

---

### Task 2: `CoreError` variants + `validate_striped_shrink`

**Files:**
- Modify: `crates/r3sizer-core/src/lib.rs:61-77` (CoreError enum)
- Modify: `crates/r3sizer-core/src/ingest.rs`

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `crates/r3sizer-core/src/ingest.rs`:

```rust
    use crate::CoreError;

    #[test]
    fn validate_rejects_small_shrink_ratio() {
        let err = validate_striped_shrink(
            ImageSize { width: 6000, height: 4000 },
            ImageSize { width: 3000, height: 2000 }, // ratio 2.0 < 3.0
        )
        .unwrap_err();
        assert!(matches!(
            err,
            CoreError::TargetTooCloseForStripedIngest { .. }
        ));
    }

    #[test]
    fn validate_accepts_ratio_at_threshold() {
        // ratio exactly 3.0 is allowed (matches the staged-shrink branch: >= 3.0).
        validate_striped_shrink(
            ImageSize { width: 6000, height: 4000 },
            ImageSize { width: 2000, height: 4000 },
        )
        .unwrap();
    }

    #[test]
    fn validate_rejects_zero_dimensions() {
        let err = validate_striped_shrink(
            ImageSize { width: 0, height: 4000 },
            ImageSize { width: 100, height: 100 },
        )
        .unwrap_err();
        assert!(matches!(err, CoreError::EmptyImage));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p r3sizer-core ingest::`
Expected: FAIL — `validate_striped_shrink` not found, `TargetTooCloseForStripedIngest` not found.

- [ ] **Step 3: Implement**

Add three variants to the `CoreError` enum in `crates/r3sizer-core/src/lib.rs` (after the `EmptyImage` variant):

```rust
    #[error("target too close to source for large-image mode (shrink ratio {ratio:.2} < 3.0)")]
    TargetTooCloseForStripedIngest { ratio: f64 },

    #[error("ingest stripe overflow: pushed {pushed} rows but only {remaining} source rows remain")]
    IngestRowOverflow { pushed: u32, remaining: u32 },

    #[error("ingest incomplete: {supplied} of {expected} source rows supplied at finish()")]
    IngestIncomplete { supplied: u32, expected: u32 },
```

Add to `crates/r3sizer-core/src/ingest.rs` (below `compute_intermediate_size`):

```rust
use crate::CoreError;

/// Check that an image qualifies for striped ingest: the shrink ratio must
/// be >= the staged-shrink threshold (3.0), otherwise the pre-reduce to
/// ~2x target is meaningless and the caller should use the monolithic path.
pub fn validate_striped_shrink(src: ImageSize, target: ImageSize) -> Result<(), CoreError> {
    if src.width == 0 || src.height == 0 || target.width == 0 || target.height == 0 {
        return Err(CoreError::EmptyImage);
    }
    let shrink_x = src.width as f64 / target.width as f64;
    let shrink_y = src.height as f64 / target.height as f64;
    let max_shrink = shrink_x.max(shrink_y);
    if max_shrink < crate::resize::STAGED_SHRINK_THRESHOLD {
        return Err(CoreError::TargetTooCloseForStripedIngest { ratio: max_shrink });
    }
    Ok(())
}
```

(The `use crate::CoreError;` in the test module from Step 1 becomes redundant — move it to the top-level imports of `ingest.rs` instead: `use crate::{types::ImageSize, CoreError};` and drop the duplicate.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p r3sizer-core ingest::`
Expected: PASS.

- [ ] **Step 5: Lint and commit**

```bash
cargo clippy --workspace -- -D warnings
git add crates/r3sizer-core/src/lib.rs crates/r3sizer-core/src/ingest.rs
git commit -m "feat(core): add striped-ingest error variants and shrink-ratio validation"
```

---

### Task 3: Move the 256-entry sRGB u8→linear LUT into core

`r3sizer-wasm/src/convert.rs:12-29` has an exact const-built `SRGB_U8_TO_LINEAR` LUT. The reducer needs it in core; move it and make wasm reuse it (DRY).

**Files:**
- Modify: `crates/r3sizer-core/src/color.rs`
- Modify: `crates/r3sizer-wasm/src/convert.rs`

- [ ] **Step 1: Write the failing test**

Append to the existing `#[cfg(test)] mod tests` in `crates/r3sizer-core/src/color.rs`:

```rust
    #[test]
    fn u8_lut_matches_reference_conversion() {
        for i in 0..=255usize {
            let expected = srgb_to_linear(i as f32 / 255.0);
            let got = SRGB_U8_TO_LINEAR[i];
            assert!(
                (got - expected).abs() < 1e-6,
                "LUT[{i}] = {got}, expected {expected}"
            );
        }
        assert_eq!(SRGB_U8_TO_LINEAR[0], 0.0);
        assert_eq!(SRGB_U8_TO_LINEAR[255], 1.0);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p r3sizer-core u8_lut`
Expected: FAIL with "cannot find value `SRGB_U8_TO_LINEAR`".

- [ ] **Step 3: Implement in core, refactor wasm**

Add to `crates/r3sizer-core/src/color.rs` (near the existing `SRGB_TO_LINEAR_LUT`; `const_pow_2_4` already exists in this file):

```rust
/// Precomputed `srgb_to_linear(i / 255.0)` for i in 0..=255 (exact, const-built).
///
/// Used for u8 ingress paths (canvas `getImageData`, striped ingest) — no
/// interpolation and no `powf` calls.
pub static SRGB_U8_TO_LINEAR: [f32; 256] = {
    let mut lut = [0.0_f32; 256];
    let mut i: usize = 0;
    while i < 256 {
        let v = i as f64 / 255.0;
        let linear = if v <= 0.04045 {
            v / 12.92
        } else {
            // exp(2.4 * ln((v + 0.055) / 1.055))
            let base = (v + 0.055) / 1.055;
            const_pow_2_4(base)
        };
        lut[i] = linear as f32;
        i += 1;
    }
    lut
};
```

In `crates/r3sizer-wasm/src/convert.rs`: delete the local `SRGB_U8_TO_LINEAR` static (lines 12-29) and add to its imports:

```rust
use r3sizer_core::color::SRGB_U8_TO_LINEAR;
```

- [ ] **Step 4: Run tests and build both crates**

Run: `cargo test -p r3sizer-core u8_lut && cargo build -p r3sizer-wasm`
Expected: test PASS, wasm crate builds.

- [ ] **Step 5: Lint and commit**

```bash
cargo clippy --workspace -- -D warnings
git add crates/r3sizer-core/src/color.rs crates/r3sizer-wasm/src/convert.rs
git commit -m "refactor: move sRGB u8->linear LUT from wasm into core color module"
```

---

### Task 4: Axis weight precomputation

Each source pixel overlaps 1–2 destination cells per axis (the footprint width `scale = dst/src < 1`, except `scale == 1` when intermediate == source, where boundaries align exactly). Precompute the mapping once so per-pixel work is pure multiply-adds.

**Files:**
- Modify: `crates/r3sizer-core/src/ingest.rs`

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `ingest.rs`:

```rust
    #[test]
    fn axis_weights_even_division() {
        // 4 source -> 2 dst, scale 0.5: pixels 0,1 -> cell 0; pixels 2,3 -> cell 1.
        let aw = axis_weights(4, 2);
        assert_eq!(aw.spans, vec![(0, 0.5, 0.0), (0, 0.5, 0.0), (1, 0.5, 0.0), (1, 0.5, 0.0)]);
    }

    #[test]
    fn axis_weights_uneven_split_pixel() {
        // 5 source -> 2 dst, scale 0.4: source pixel 2 straddles the boundary.
        let aw = axis_weights(5, 2);
        assert_eq!(aw.spans[2].0, 0);
        assert!((aw.spans[2].1 - 0.2).abs() < 1e-6); // into cell 0
        assert!((aw.spans[2].2 - 0.2).abs() < 1e-6); // into cell 1
    }

    #[test]
    fn axis_weights_conservation() {
        // For any (src, dst): each source span sums to `scale`, and the total
        // weight landing in each destination cell is exactly 1.0 (its width
        // in destination space).
        for (src, dst) in [(4u32, 2u32), (5, 2), (97, 13), (1000, 333), (7, 7)] {
            let scale = dst as f64 / src as f64;
            let aw = axis_weights(src, dst);
            let mut per_cell = vec![0.0f64; dst as usize];
            for &(j0, w0, w1) in &aw.spans {
                assert!((w0 as f64 + w1 as f64 - scale).abs() < 1e-9);
                per_cell[j0 as usize] += w0 as f64;
                if w1 > 0.0 {
                    per_cell[j0 as usize + 1] += w1 as f64;
                }
            }
            for (j, total) in per_cell.iter().enumerate() {
                assert!(
                    (total - 1.0).abs() < 1e-6,
                    "src={src} dst={dst} cell {j}: total weight {total}"
                );
            }
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p r3sizer-core ingest::tests::axis`
Expected: FAIL with "cannot find function `axis_weights`".

- [ ] **Step 3: Implement**

Add to `ingest.rs` (above the tests module):

```rust
/// Per-source-index destination mapping for one axis.
///
/// `spans[i]` = (first destination index, weight into it, weight into the
/// *next* destination index — 0.0 when the source pixel does not straddle a
/// cell boundary). Weights are measured in destination-space length, so each
/// span's weights sum to `scale = dst_len / src_len`.
#[derive(Debug)]
struct AxisWeights {
    spans: Vec<(u32, f32, f32)>,
}

fn axis_weights(src_len: u32, dst_len: u32) -> AxisWeights {
    debug_assert!(dst_len >= 1 && dst_len <= src_len);
    let scale = dst_len as f64 / src_len as f64;
    let mut spans = Vec::with_capacity(src_len as usize);
    for i in 0..src_len {
        let start = i as f64 * scale;
        let end = (i as f64 + 1.0) * scale;
        let j0 = (start.floor() as u32).min(dst_len - 1);
        let boundary = (j0 + 1) as f64;
        if end <= boundary + 1e-12 || j0 + 1 >= dst_len {
            spans.push((j0, (end - start) as f32, 0.0));
        } else {
            spans.push((j0, (boundary - start) as f32, (end - boundary) as f32));
        }
    }
    AxisWeights { spans }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p r3sizer-core ingest::`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/r3sizer-core/src/ingest.rs
git commit -m "feat(core): area-weight axis mapping for striped pre-reduce"
```

---

### Task 5: `StripedPreReducer` — construction and push validation

**Files:**
- Modify: `crates/r3sizer-core/src/ingest.rs`

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module:

```rust
    fn reducer_4x4_to_2x2() -> StripedPreReducer {
        StripedPreReducer::new(
            ImageSize { width: 4, height: 4 },
            ImageSize { width: 2, height: 2 },
        )
        .unwrap()
    }

    #[test]
    fn new_rejects_zero_dimensions() {
        let err = StripedPreReducer::new(
            ImageSize { width: 0, height: 4 },
            ImageSize { width: 2, height: 2 },
        )
        .unwrap_err();
        assert!(matches!(err, CoreError::EmptyImage));
    }

    #[test]
    fn new_rejects_intermediate_larger_than_source() {
        let err = StripedPreReducer::new(
            ImageSize { width: 4, height: 4 },
            ImageSize { width: 8, height: 2 },
        )
        .unwrap_err();
        assert!(matches!(err, CoreError::InvalidParams(_)));
    }

    #[test]
    fn push_rejects_wrong_buffer_length() {
        let mut r = reducer_4x4_to_2x2();
        // 2 rows of a 4-wide image need 4*2*4 = 32 bytes; give 31.
        let err = r.push_srgb8_rows(&[0u8; 31], 2).unwrap_err();
        assert!(matches!(err, CoreError::BufferLengthMismatch { .. }));
    }

    #[test]
    fn push_rejects_zero_rows() {
        let mut r = reducer_4x4_to_2x2();
        let err = r.push_srgb8_rows(&[], 0).unwrap_err();
        assert!(matches!(err, CoreError::InvalidParams(_)));
    }

    #[test]
    fn push_rejects_row_overflow() {
        let mut r = reducer_4x4_to_2x2();
        r.push_srgb8_rows(&[128u8; 4 * 3 * 4], 3).unwrap();
        // 3 of 4 rows consumed; pushing 2 more overflows.
        let err = r.push_srgb8_rows(&[128u8; 4 * 2 * 4], 2).unwrap_err();
        assert!(matches!(
            err,
            CoreError::IngestRowOverflow { pushed: 2, remaining: 1 }
        ));
    }

    #[test]
    fn finish_rejects_incomplete_supply() {
        let mut r = reducer_4x4_to_2x2();
        r.push_srgb8_rows(&[128u8; 4 * 2 * 4], 2).unwrap();
        let err = r.finish().unwrap_err();
        assert!(matches!(
            err,
            CoreError::IngestIncomplete { supplied: 2, expected: 4 }
        ));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p r3sizer-core ingest::`
Expected: FAIL — `StripedPreReducer` not found.

- [ ] **Step 3: Implement the struct (accumulation logic included — correctness tests come in Task 6)**

Add to `ingest.rs`:

```rust
use crate::color::SRGB_U8_TO_LINEAR;
use crate::types::LinearRgbImage;

/// Streaming area-weighted pre-reducer.
///
/// Feed full-width sRGB RGBA8 row stripes strictly top-to-bottom via
/// [`push_srgb8_rows`](Self::push_srgb8_rows), then call
/// [`finish`](Self::finish) to obtain the linear intermediate image.
/// Alpha is ignored (input is treated as straight, non-premultiplied RGBA).
pub struct StripedPreReducer {
    src: ImageSize,
    intermediate: ImageSize,
    x_weights: AxisWeights,
    y_weights: AxisWeights,
    /// Interleaved RGB accumulators, len = inter_w * inter_h * 3.
    acc: Vec<f32>,
    /// Per-pixel weight accumulator, len = inter_w * inter_h. Normalizing by
    /// the accumulated weight in `finish()` avoids edge errors when source
    /// dimensions do not divide evenly into the intermediate.
    weight: Vec<f32>,
    next_row: u32,
}

impl StripedPreReducer {
    /// `intermediate` must come from [`compute_intermediate_size`] (same
    /// logic as the staged-shrink path, ~2x the final target).
    pub fn new(src: ImageSize, intermediate: ImageSize) -> Result<Self, CoreError> {
        if src.width == 0 || src.height == 0 || intermediate.width == 0 || intermediate.height == 0
        {
            return Err(CoreError::EmptyImage);
        }
        if intermediate.width > src.width || intermediate.height > src.height {
            return Err(CoreError::InvalidParams(format!(
                "intermediate {}x{} exceeds source {}x{}",
                intermediate.width, intermediate.height, src.width, src.height
            )));
        }
        let n = intermediate.width as usize * intermediate.height as usize;
        Ok(Self {
            src,
            intermediate,
            x_weights: axis_weights(src.width, intermediate.width),
            y_weights: axis_weights(src.height, intermediate.height),
            acc: vec![0.0; n * 3],
            weight: vec![0.0; n],
            next_row: 0,
        })
    }

    pub fn source_size(&self) -> ImageSize {
        self.src
    }

    pub fn intermediate_size(&self) -> ImageSize {
        self.intermediate
    }

    pub fn rows_pushed(&self) -> u32 {
        self.next_row
    }

    /// Push the next `rows` full-width rows of sRGB RGBA8 pixels.
    ///
    /// Rows are consumed strictly sequentially top-to-bottom; supplying more
    /// rows than remain in the source is an error.
    pub fn push_srgb8_rows(&mut self, rgba: &[u8], rows: u32) -> Result<(), CoreError> {
        if rows == 0 {
            return Err(CoreError::InvalidParams("stripe with zero rows".into()));
        }
        let remaining = self.src.height - self.next_row;
        if rows > remaining {
            return Err(CoreError::IngestRowOverflow { pushed: rows, remaining });
        }
        let row_bytes = self.src.width as usize * 4;
        let expected_len = row_bytes * rows as usize;
        if rgba.len() != expected_len {
            return Err(CoreError::BufferLengthMismatch {
                expected_len,
                got_len: rgba.len(),
            });
        }

        let iw = self.intermediate.width as usize;
        for r in 0..rows as usize {
            let y = self.next_row as usize + r;
            let (jy, wy0, wy1) = self.y_weights.spans[y];
            let row = &rgba[r * row_bytes..(r + 1) * row_bytes];
            for (x, px) in row.chunks_exact(4).enumerate() {
                let lr = SRGB_U8_TO_LINEAR[px[0] as usize];
                let lg = SRGB_U8_TO_LINEAR[px[1] as usize];
                let lb = SRGB_U8_TO_LINEAR[px[2] as usize];
                let (jx, wx0, wx1) = self.x_weights.spans[x];
                // Up to 2x2 destination cells; second row/col weight may be 0.
                accumulate(&mut self.acc, &mut self.weight, iw, jx, jy, wx0 * wy0, lr, lg, lb);
                if wx1 > 0.0 {
                    accumulate(&mut self.acc, &mut self.weight, iw, jx + 1, jy, wx1 * wy0, lr, lg, lb);
                }
                if wy1 > 0.0 {
                    accumulate(&mut self.acc, &mut self.weight, iw, jx, jy + 1, wx0 * wy1, lr, lg, lb);
                    if wx1 > 0.0 {
                        accumulate(&mut self.acc, &mut self.weight, iw, jx + 1, jy + 1, wx1 * wy1, lr, lg, lb);
                    }
                }
            }
        }
        self.next_row += rows;
        Ok(())
    }

    /// Normalize the accumulators into the linear intermediate image.
    ///
    /// Errors if not all source rows were supplied.
    pub fn finish(self) -> Result<LinearRgbImage, CoreError> {
        if self.next_row != self.src.height {
            return Err(CoreError::IngestIncomplete {
                supplied: self.next_row,
                expected: self.src.height,
            });
        }
        let mut data = self.acc;
        for (i, w) in self.weight.iter().enumerate() {
            // Every cell received weight: per-cell totals are exactly 1.0
            // per axis (see axis_weights_conservation test).
            let inv = 1.0 / w;
            data[i * 3] *= inv;
            data[i * 3 + 1] *= inv;
            data[i * 3 + 2] *= inv;
        }
        LinearRgbImage::new(self.intermediate.width, self.intermediate.height, data)
    }
}

#[inline]
fn accumulate(
    acc: &mut [f32],
    weight: &mut [f32],
    iw: usize,
    jx: u32,
    jy: u32,
    w: f32,
    r: f32,
    g: f32,
    b: f32,
) {
    let idx = jy as usize * iw + jx as usize;
    acc[idx * 3] += w * r;
    acc[idx * 3 + 1] += w * g;
    acc[idx * 3 + 2] += w * b;
    weight[idx] += w;
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p r3sizer-core ingest::`
Expected: PASS.

- [ ] **Step 5: Lint and commit**

```bash
cargo clippy --workspace -- -D warnings
git add crates/r3sizer-core/src/ingest.rs
git commit -m "feat(core): StripedPreReducer with sequential stripe validation"
```

---

### Task 6: Accumulation correctness on synthetic images

**Files:**
- Modify: `crates/r3sizer-core/src/ingest.rs` (tests only)

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module:

```rust
    /// Build an RGBA8 buffer from a per-pixel (r, g, b) function.
    fn rgba_from_fn(w: u32, h: u32, f: impl Fn(u32, u32) -> (u8, u8, u8)) -> Vec<u8> {
        let mut data = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let (r, g, b) = f(x, y);
                data.extend_from_slice(&[r, g, b, 255]);
            }
        }
        data
    }

    fn reduce_whole(src: ImageSize, inter: ImageSize, rgba: &[u8]) -> LinearRgbImage {
        let mut r = StripedPreReducer::new(src, inter).unwrap();
        r.push_srgb8_rows(rgba, src.height).unwrap();
        r.finish().unwrap()
    }

    #[test]
    fn solid_color_reduces_to_same_color() {
        let src = ImageSize { width: 10, height: 7 };
        let inter = ImageSize { width: 3, height: 2 };
        let rgba = rgba_from_fn(10, 7, |_, _| (128, 64, 200));
        let out = reduce_whole(src, inter, &rgba);
        let expected = [
            SRGB_U8_TO_LINEAR[128],
            SRGB_U8_TO_LINEAR[64],
            SRGB_U8_TO_LINEAR[200],
        ];
        for px in out.pixels().chunks_exact(3) {
            for c in 0..3 {
                assert!((px[c] - expected[c]).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn checkerboard_reduces_to_exact_mean() {
        // 4x4 1px checkerboard (0 / 255) -> 2x2: each output cell averages
        // exactly two black and two white pixels in linear space.
        let src = ImageSize { width: 4, height: 4 };
        let inter = ImageSize { width: 2, height: 2 };
        let rgba = rgba_from_fn(4, 4, |x, y| {
            let v = if (x + y) % 2 == 0 { 255 } else { 0 };
            (v, v, v)
        });
        let out = reduce_whole(src, inter, &rgba);
        let expected = (SRGB_U8_TO_LINEAR[255] + SRGB_U8_TO_LINEAR[0]) / 2.0;
        for px in out.pixels() {
            assert!((px - expected).abs() < 1e-6, "got {px}, expected {expected}");
        }
    }

    #[test]
    fn row_gradient_reduces_to_exact_row_means() {
        // Each source row is constant; 4 rows -> 2 output rows, so each
        // output row is the exact mean of two LUT values.
        let src = ImageSize { width: 4, height: 4 };
        let inter = ImageSize { width: 2, height: 2 };
        let values = [10u8, 80, 160, 240];
        let rgba = rgba_from_fn(4, 4, |_, y| {
            let v = values[y as usize];
            (v, v, v)
        });
        let out = reduce_whole(src, inter, &rgba);
        let expect_row0 = (SRGB_U8_TO_LINEAR[10] + SRGB_U8_TO_LINEAR[80]) / 2.0;
        let expect_row1 = (SRGB_U8_TO_LINEAR[160] + SRGB_U8_TO_LINEAR[240]) / 2.0;
        for px in out.row(0) {
            assert!((px - expect_row0).abs() < 1e-6);
        }
        for px in out.row(1) {
            assert!((px - expect_row1).abs() < 1e-6);
        }
    }
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test -p r3sizer-core ingest::`
Expected: PASS — Task 5 implemented the accumulation. If any fail, fix the accumulation (not the tests): the most likely bugs are wrong index arithmetic in `accumulate` or weights not summing per-cell.

- [ ] **Step 3: Commit**

```bash
git add crates/r3sizer-core/src/ingest.rs
git commit -m "test(core): synthetic-image exactness tests for striped pre-reduce"
```

---

### Task 7: Slicing invariance and parity with the staged bilinear pre-reduce

**Files:**
- Modify: `crates/r3sizer-core/src/ingest.rs` (tests only)

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module:

```rust
    /// Deterministic pseudo-detail pattern (no RNG: reproducible).
    fn patterned_rgba(w: u32, h: u32) -> Vec<u8> {
        rgba_from_fn(w, h, |x, y| {
            let r = ((x * 7 + y * 13) % 256) as u8;
            let g = ((x * 3) % 200) as u8;
            let b = ((y * 5 + 31) % 256) as u8;
            (r, g, b)
        })
    }

    fn reduce_in_stripes(
        src: ImageSize,
        inter: ImageSize,
        rgba: &[u8],
        stripe_rows: u32,
    ) -> LinearRgbImage {
        let mut r = StripedPreReducer::new(src, inter).unwrap();
        let row_bytes = src.width as usize * 4;
        let mut y = 0u32;
        while y < src.height {
            let rows = stripe_rows.min(src.height - y);
            let start = y as usize * row_bytes;
            let end = start + rows as usize * row_bytes;
            r.push_srgb8_rows(&rgba[start..end], rows).unwrap();
            y += rows;
        }
        r.finish().unwrap()
    }

    #[test]
    fn result_is_independent_of_stripe_slicing() {
        let src = ImageSize { width: 97, height: 61 };
        let target = ImageSize { width: 13, height: 11 };
        let inter = compute_intermediate_size(src, target);
        let rgba = patterned_rgba(src.width, src.height);

        let whole = reduce_whole(src, inter, &rgba);
        for stripe_rows in [1u32, 17, 32, 61] {
            let sliced = reduce_in_stripes(src, inter, &rgba, stripe_rows);
            // Accumulation order is row-major regardless of stripe
            // boundaries, so results are bit-identical.
            assert_eq!(
                whole.pixels(),
                sliced.pixels(),
                "stripe_rows={stripe_rows} changed the result"
            );
        }
    }

    #[test]
    fn smooth_gradient_close_to_staged_bilinear_prereduce() {
        // On smooth content the area average and the staged path's bilinear
        // pre-reduce agree closely. (They are different filters — the spec
        // explicitly allows numerical differences; sharp content diverges
        // more, which the pipeline-level test in tests/striped_ingest.rs
        // covers with an s* tolerance instead.)
        let src = ImageSize { width: 600, height: 400 };
        let target = ImageSize { width: 100, height: 66 };
        let inter = compute_intermediate_size(src, target);

        let rgba = rgba_from_fn(600, 400, |x, y| {
            (
                (x as f32 / 599.0 * 255.0) as u8,
                (y as f32 / 399.0 * 255.0) as u8,
                128,
            )
        });

        let striped = reduce_in_stripes(src, inter, &rgba, 37);

        // Reference: same quantized pixels, linearized identically, then the
        // staged path's bilinear pre-reduce.
        let mut linear = Vec::with_capacity((src.width * src.height * 3) as usize);
        for px in rgba.chunks_exact(4) {
            linear.push(SRGB_U8_TO_LINEAR[px[0] as usize]);
            linear.push(SRGB_U8_TO_LINEAR[px[1] as usize]);
            linear.push(SRGB_U8_TO_LINEAR[px[2] as usize]);
        }
        let full = LinearRgbImage::new(src.width, src.height, linear).unwrap();
        let bilinear = crate::resize::fir_resize(
            &full,
            inter.width,
            inter.height,
            fast_image_resize::ResizeAlg::Convolution(fast_image_resize::FilterType::Bilinear),
        )
        .unwrap();

        let mut max_diff = 0.0f32;
        for (a, b) in striped.pixels().iter().zip(bilinear.pixels()) {
            max_diff = max_diff.max((a - b).abs());
        }
        assert!(max_diff < 1e-2, "max per-channel diff {max_diff} too large");
    }
```

Note: check how `resize.rs` imports the `fast_image_resize` crate (it aliases it as `fir`). Match the test's qualified paths to whatever the crate is named in `Cargo.toml` (`fast_image_resize`); if `cargo test` reports an unresolved import, mirror the exact `use` from `resize.rs` instead.

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test -p r3sizer-core ingest::`
Expected: PASS. If `smooth_gradient_close_to_staged_bilinear_prereduce` fails on tolerance only (max diff between 1e-2 and 5e-2), the implementations disagree on edge handling — inspect the first/last row/column diffs before loosening anything.

- [ ] **Step 3: Commit**

```bash
git add crates/r3sizer-core/src/ingest.rs
git commit -m "test(core): stripe-slicing invariance and bilinear pre-reduce parity"
```

---

### Task 8: `IngestDiagnostics` type, exports, and TypeScript regeneration

**Files:**
- Modify: `crates/r3sizer-core/src/types.rs`
- Modify: `crates/r3sizer-core/src/pipeline.rs` (diagnostics literal, ~line 1000-1048)
- Modify: `crates/r3sizer-core/src/lib.rs` (exports)
- Modify: `crates/r3sizer-core/src/prelude.rs`
- Modify: `crates/r3sizer-core/tests/typegen.rs`
- Regenerate: `web/src/types/generated.ts`

- [ ] **Step 1: Add the type to `types.rs`**

Add near the other diagnostics types (e.g. after `InputIngressDiagnostics`):

```rust
/// Diagnostics for the striped (streaming) ingest path used for very large
/// images. Present only when the input was ingested via
/// [`crate::ingest::StripedPreReducer`]; the pipeline's `input_size` then
/// refers to the intermediate image, and the original source dimensions are
/// recorded here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typegen", derive(TS))]
pub struct IngestDiagnostics {
    pub original_width: u32,
    pub original_height: u32,
    pub intermediate_width: u32,
    pub intermediate_height: u32,
    pub striped: bool,
    /// Content-adaptive resize needs the full source; forced to uniform.
    pub forced_uniform_resize: bool,
    /// `full_diagnostics` source-side metrics need the full source; skipped.
    pub skipped_source_diagnostics: bool,
}
```

Add the field to `AutoSharpDiagnostics` (after the `used_staged_shrink` field):

```rust
    // --- Striped ingest ---
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ingest: Option<IngestDiagnostics>,
```

- [ ] **Step 2: Fix all construction sites**

In `crates/r3sizer-core/src/pipeline.rs`, add to the `AutoSharpDiagnostics { ... }` literal in `finish_pipeline` (after `used_staged_shrink: ...`):

```rust
        ingest: None,
```

Run `cargo build --workspace` — the compiler flags any other construction site; add `ingest: None` to each.

- [ ] **Step 3: Export from `lib.rs` and `prelude.rs`**

In `lib.rs`, add `IngestDiagnostics` to the `pub use types::{...}` list (alphabetical: after `ImageSize, InputColorSpace,` → `... ImageSize, IngestDiagnostics, InputColorSpace, ...`).

In `prelude.rs`, extend the diagnostics re-export line:

```rust
pub use crate::{AutoSharpDiagnostics, IngestDiagnostics, ProcessOutput, StageTiming};
```

Also re-export the ingest API in `lib.rs` (after the existing `pub use pipeline::{...}` block):

```rust
pub use ingest::{compute_intermediate_size, validate_striped_shrink, StripedPreReducer};
```

- [ ] **Step 4: Add to typegen and regenerate**

In `crates/r3sizer-core/tests/typegen.rs`: add `IngestDiagnostics` to the `use r3sizer_core::{...}` import list, and add the declaration **before** `AutoSharpDiagnostics::decl(&cfg),` in the first declarations vec:

```rust
        IngestDiagnostics::decl(&cfg),
        AutoSharpDiagnostics::decl(&cfg),
```

Run: `cargo test -p r3sizer-core --features typegen export_typescript_bindings -- --nocapture`
Expected: `✓ Wrote .../web/src/types/generated.ts`. Verify with `grep -n "IngestDiagnostics" web/src/types/generated.ts` — the type and the `ingest?` field on `AutoSharpDiagnostics` must both appear.

- [ ] **Step 5: Test, lint, commit**

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
git add crates/r3sizer-core/src/types.rs crates/r3sizer-core/src/pipeline.rs crates/r3sizer-core/src/lib.rs crates/r3sizer-core/src/prelude.rs crates/r3sizer-core/tests/typegen.rs web/src/types/generated.ts
git commit -m "feat(core): IngestDiagnostics threaded into AutoSharpDiagnostics + TS regen"
```

---

### Task 9: Pipeline-parity integration test (striped vs. monolithic s*)

**Files:**
- Create: `crates/r3sizer-core/tests/striped_ingest.rs`

- [ ] **Step 1: Write the test**

```rust
//! End-to-end parity: a "large" image processed via striped ingest selects a
//! sharpening strength close to the monolithic staged path. The intermediates
//! differ (area average vs. bilinear pre-reduce), so s* is compared with a
//! tolerance, not exactly.

use r3sizer_core::color::SRGB_U8_TO_LINEAR;
use r3sizer_core::{
    compute_intermediate_size, process_auto_sharp_downscale, AutoSharpParams, ImageSize,
    LinearRgbImage, StripedPreReducer,
};

/// Gradient + checker detail so probing has artifacts to measure.
fn synthetic_rgba(w: u32, h: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let base = (x * 200 / w.max(1)) as u8;
            let checker = if (x / 3 + y / 3) % 2 == 0 { 55 } else { 0 };
            let r = base.saturating_add(checker);
            let g = 200u8.saturating_sub(base);
            let b = (y * 200 / h.max(1)) as u8;
            data.extend_from_slice(&[r, g, b, 255]);
        }
    }
    data
}

fn linear_from_rgba(rgba: &[u8], w: u32, h: u32) -> LinearRgbImage {
    let mut out = Vec::with_capacity((w * h * 3) as usize);
    for px in rgba.chunks_exact(4) {
        out.push(SRGB_U8_TO_LINEAR[px[0] as usize]);
        out.push(SRGB_U8_TO_LINEAR[px[1] as usize]);
        out.push(SRGB_U8_TO_LINEAR[px[2] as usize]);
    }
    LinearRgbImage::new(w, h, out).unwrap()
}

#[test]
fn striped_ingest_selects_similar_strength_to_monolithic() {
    let (w, h) = (2400u32, 1600u32);
    let rgba = synthetic_rgba(w, h);
    let src = ImageSize { width: w, height: h };
    let target = ImageSize { width: 400, height: 267 }; // ratio 6 -> staged path

    let mut params = AutoSharpParams::default();
    params.target_width = target.width;
    params.target_height = target.height;

    // Monolithic reference, built from the SAME quantized pixels.
    let full = linear_from_rgba(&rgba, w, h);
    let mono = process_auto_sharp_downscale(&full, &params).unwrap();

    // Striped: feed 64-row stripes through the reducer, then the same pipeline.
    let inter_size = compute_intermediate_size(src, target);
    let mut reducer = StripedPreReducer::new(src, inter_size).unwrap();
    let row_bytes = (w as usize) * 4;
    for chunk in rgba.chunks(row_bytes * 64) {
        let rows = (chunk.len() / row_bytes) as u32;
        reducer.push_srgb8_rows(chunk, rows).unwrap();
    }
    let intermediate = reducer.finish().unwrap();
    assert_eq!(intermediate.width(), inter_size.width);
    assert_eq!(intermediate.height(), inter_size.height);

    let striped = process_auto_sharp_downscale(&intermediate, &params).unwrap();

    assert_eq!(striped.image.width(), target.width);
    assert_eq!(striped.image.height(), target.height);

    let s_mono = mono.diagnostics.selected_strength;
    let s_striped = striped.diagnostics.selected_strength;
    assert!(
        (s_striped - s_mono).abs() <= 0.3,
        "selected strength diverged: monolithic={s_mono}, striped={s_striped}"
    );
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p r3sizer-core --test striped_ingest -- --nocapture`
Expected: PASS (takes a few seconds — it runs the full pipeline twice on a 2400×1600 image). If the s* assertion fails, print both diagnostics' `selection_mode` and `probe_samples` — a divergence usually means the two intermediates differ structurally (bug), not just numerically; do **not** loosen the tolerance without confirming both paths used the same selection mode.

- [ ] **Step 3: Commit**

```bash
git add crates/r3sizer-core/tests/striped_ingest.rs
git commit -m "test(core): striped vs monolithic pipeline strength parity"
```

---

### Task 10: WASM ingest exports and cache integration

**Files:**
- Modify: `crates/r3sizer-wasm/src/lib.rs`

No native unit tests are possible for `wasm_bindgen` exports returning `JsValue`; the logic lives in core (already tested). Verification here is compile + clippy + the wasm-pack build, plus the facade tests in Task 17.

- [ ] **Step 1: Add thread-locals and imports**

In `crates/r3sizer-wasm/src/lib.rs`, extend the imports to include `DiagnosticsLevel`, `ImageSize`, `IngestDiagnostics`, and `StripedPreReducer` (merge into the existing `use r3sizer_core::{...}` line), and extend the `thread_local!` block (lib.rs:19-22):

```rust
thread_local! {
    static CACHED_INPUT: RefCell<Option<LinearRgbImage>> = const { RefCell::new(None) };
    static CACHED_BASE: RefCell<Option<r3sizer_core::PreparedBase>> = const { RefCell::new(None) };
    static CACHED_INGEST: RefCell<Option<StripedPreReducer>> = const { RefCell::new(None) };
    static CACHED_INGEST_DIAG: RefCell<Option<IngestDiagnostics>> = const { RefCell::new(None) };
}
```

- [ ] **Step 2: Add the four exports**

```rust
/// Begin a striped ingest for a large image. Validates the shrink ratio
/// (>= 3x), computes the intermediate size, and invalidates all caches from
/// the previous image. Returns `{ width, height }` of the intermediate.
#[wasm_bindgen]
pub fn ingest_begin(
    src_w: u32,
    src_h: u32,
    target_w: u32,
    target_h: u32,
) -> Result<JsValue, JsValue> {
    let src = ImageSize { width: src_w, height: src_h };
    let target = ImageSize { width: target_w, height: target_h };
    r3sizer_core::validate_striped_shrink(src, target)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    let inter = r3sizer_core::compute_intermediate_size(src, target);
    let reducer = StripedPreReducer::new(src, inter)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    // A new striped image invalidates everything cached for the previous one.
    CACHED_INPUT.with(|c| *c.borrow_mut() = None);
    CACHED_BASE.with(|c| *c.borrow_mut() = None);
    CACHED_INGEST_DIAG.with(|c| *c.borrow_mut() = None);
    CACHED_INGEST.with(|c| *c.borrow_mut() = Some(reducer));

    dims_object(inter)
}

/// Feed the next full-width stripe (strictly sequential, top-to-bottom).
/// On any error the partial ingest state is dropped.
#[wasm_bindgen]
pub fn ingest_stripe(rgba: &[u8], rows: u32) -> Result<(), JsValue> {
    let result = CACHED_INGEST.with(|c| {
        let mut cache = c.borrow_mut();
        match cache.as_mut() {
            None => Err("no active ingest — call ingest_begin first".to_string()),
            Some(reducer) => reducer.push_srgb8_rows(rgba, rows).map_err(|e| e.to_string()),
        }
    });
    if let Err(msg) = result {
        ingest_abort();
        return Err(JsValue::from_str(&msg));
    }
    Ok(())
}

/// Finish the ingest: the intermediate becomes the cached input image (same
/// thread-local that `prepare_image` fills), so the entire downstream
/// protocol works unchanged. Returns `{ width, height }` of the intermediate.
#[wasm_bindgen]
pub fn ingest_end() -> Result<JsValue, JsValue> {
    let reducer = CACHED_INGEST
        .with(|c| c.borrow_mut().take())
        .ok_or_else(|| JsValue::from_str("no active ingest — call ingest_begin first"))?;
    let src = reducer.source_size();
    let inter = reducer.intermediate_size();
    let intermediate = reducer
        .finish()
        .map_err(|e| JsValue::from_str(&e.to_string()))?;

    CACHED_INPUT.with(|c| *c.borrow_mut() = Some(intermediate));
    CACHED_INGEST_DIAG.with(|c| {
        *c.borrow_mut() = Some(IngestDiagnostics {
            original_width: src.width,
            original_height: src.height,
            intermediate_width: inter.width,
            intermediate_height: inter.height,
            striped: true,
            forced_uniform_resize: true,
            skipped_source_diagnostics: true,
        })
    });

    dims_object(inter)
}

/// Drop partial ingest state (used by cancellation). Idempotent.
#[wasm_bindgen]
pub fn ingest_abort() {
    CACHED_INGEST.with(|c| *c.borrow_mut() = None);
}

fn dims_object(size: ImageSize) -> Result<JsValue, JsValue> {
    let result = js_sys::Object::new();
    js_sys::Reflect::set(&result, &"width".into(), &JsValue::from(size.width))?;
    js_sys::Reflect::set(&result, &"height".into(), &JsValue::from(size.height))?;
    Ok(result.into())
}
```

- [ ] **Step 3: Enforce striped constraints and inject diagnostics**

Add a helper and call it immediately after `parse_params(...)` in `prepare_base`, `process_image`, and `process_from_probes` (e.g. `let mut params = parse_params(params_json)?; apply_ingest_constraints(&mut params);`):

```rust
/// Striped-path constraints: content-adaptive resize classifies the full
/// source and full diagnostics need source-side metrics — neither exists on
/// the striped path, so force uniform resize and summary diagnostics.
fn apply_ingest_constraints(params: &mut AutoSharpParams) {
    let striped = CACHED_INGEST_DIAG.with(|c| c.borrow().is_some());
    if striped {
        params.resize_strategy = None;
        params.diagnostics_level = DiagnosticsLevel::Summary;
    }
}
```

Change `serialize_output` to inject the ingest diagnostics (lib.rs:128, make the parameter `mut`):

```rust
fn serialize_output(mut output: r3sizer_core::ProcessOutput) -> Result<JsValue, JsValue> {
    if let Some(diag) = CACHED_INGEST_DIAG.with(|c| c.borrow().clone()) {
        output.diagnostics.ingest = Some(diag);
    }
    post_progress("encoding");
    // ... rest unchanged ...
```

Extend `clear_cache` (lib.rs:78-82):

```rust
#[wasm_bindgen]
pub fn clear_cache() {
    CACHED_INPUT.with(|c| *c.borrow_mut() = None);
    CACHED_BASE.with(|c| *c.borrow_mut() = None);
    CACHED_INGEST.with(|c| *c.borrow_mut() = None);
    CACHED_INGEST_DIAG.with(|c| *c.borrow_mut() = None);
}
```

- [ ] **Step 4: Re-store the consumed input cache in `process_image`**

`get_or_convert_input` *takes* the cached input. On the striped path JS passes an **empty** `rgbaData` (the intermediate lives only in the cache), so consuming it would break every subsequent call. In `process_image` (lib.rs:178-206), after the `let output = match ...` block and before `serialize_output(output)`, put the input back:

```rust
    // Keep the input cached: the striped path has no way to re-supply it
    // (JS sends empty pixel data), and on the monolithic path re-caching
    // saves the next call's conversion.
    CACHED_INPUT.with(|c| *c.borrow_mut() = Some(input));
```

(If the existing code consumes `input` by value into the pipeline call, it doesn't — both `process_from_prepared` and `process_auto_sharp_downscale_with_progress` take `&input`.)

- [ ] **Step 5: Build, lint, commit**

```bash
cargo build -p r3sizer-wasm
cargo clippy --workspace -- -D warnings
cd web && npm run build:wasm && cd ..
git add crates/r3sizer-wasm/src/lib.rs
git commit -m "feat(wasm): striped ingest exports with cache integration and constraint enforcement"
```

---

### Task 11: Vitest setup for the web app

**Files:**
- Modify: `web/package.json`
- Create: `web/vitest.config.ts`
- Create: `web/src/processing/setup.test.ts` (smoke test, replaced by real tests in later tasks)

- [ ] **Step 1: Install and configure**

```bash
cd web && npm install --save-dev vitest happy-dom
```

Create `web/vitest.config.ts`:

```ts
import path from "node:path";
import { defineConfig } from "vitest/config";

export default defineConfig({
  resolve: {
    alias: { "@": path.resolve(__dirname, "src") },
  },
  test: {
    environment: "happy-dom",
    include: ["src/**/*.test.ts"],
  },
});
```

Add to `web/package.json` scripts:

```json
    "test": "vitest run",
```

Create `web/src/processing/setup.test.ts`:

```ts
import { describe, expect, it } from "vitest";

describe("vitest setup", () => {
  it("runs in a DOM-like environment", () => {
    expect(typeof document).toBe("object");
  });
});
```

- [ ] **Step 2: Run tests and the production type-check**

Run: `cd web && npm test && npx tsc -b`
Expected: 1 test PASS; `tsc -b` clean (test files use explicit vitest imports, so no tsconfig changes needed — if `tsc` complains about vitest types, add `"vitest"` is NOT needed; explicit imports resolve from node_modules).

- [ ] **Step 3: Commit**

```bash
git add web/package.json web/package-lock.json web/vitest.config.ts web/src/processing/setup.test.ts
git commit -m "chore(web): add vitest with happy-dom environment"
```

---

### Task 12: Move worker modules into `web/src/processing/`

Pure move + import fixes. No behavior change.

**Files:**
- Move: `web/src/wasm.ts` → `web/src/processing/wasm.ts`
- Move: `web/src/wasm-worker.ts` → `web/src/processing/wasm-worker.ts`
- Move: `web/src/probe-pool.ts` → `web/src/processing/probe-pool.ts`
- Move: `web/src/probe-worker.ts` → `web/src/processing/probe-worker.ts`
- Modify: `web/src/stores/processor-store.ts` (import path)

- [ ] **Step 1: Move**

```bash
cd web/src
git mv wasm.ts wasm-worker.ts probe-pool.ts probe-worker.ts processing/
```

- [ ] **Step 2: Fix relative imports inside the moved files**

The `wasm-pkg` directory stays at `web/src/wasm-pkg/`. Update:
- `processing/wasm.ts:5`: `import wasmUrl from "./wasm-pkg/r3sizer_wasm_bg.wasm?url"` → `"../wasm-pkg/r3sizer_wasm_bg.wasm?url"`
- `processing/wasm-worker.ts:1-6`: `from "./wasm-pkg/r3sizer_wasm"` → `from "../wasm-pkg/r3sizer_wasm"`
- `processing/probe-worker.ts`: same `wasm-pkg` path fix (`./wasm-pkg/...` → `../wasm-pkg/...`)
- Worker URLs (`new URL("./wasm-worker.ts", import.meta.url)` in wasm.ts, `new URL("./probe-worker.ts", import.meta.url)` in probe-pool.ts) stay `./` — the files moved together.

In `web/src/stores/processor-store.ts:7`: `from "@/wasm"` → `from "@/processing/wasm"`.

- [ ] **Step 3: Verify the dev build and behavior**

Run: `cd web && npx tsc -b && npm run lint`
Expected: clean. (Full `npm run build` also works but rebuilds WASM; `tsc -b` is the fast check.)

- [ ] **Step 4: Commit**

```bash
git add -A web/src
git commit -m "refactor(web): move worker modules into src/processing/"
```

---

### Task 13: `errors.ts` and `progress.ts`

**Files:**
- Create: `web/src/processing/progress.ts`
- Create: `web/src/processing/errors.ts`
- Create: `web/src/processing/progress.test.ts`
- Delete: `web/src/processing/setup.test.ts` (superseded by real tests)

- [ ] **Step 1: Write the failing tests**

Create `web/src/processing/progress.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { ProgressAggregator, type ProgressEvent } from "./progress";

describe("ProgressAggregator", () => {
  it("normalizes overall progress over the active stages", () => {
    const events: ProgressEvent[] = [];
    const agg = new ProgressAggregator(["prepare", "probe", "finalize"], (e) =>
      events.push(e),
    );
    agg.update("prepare", 1);
    agg.update("probe", 0.5);
    agg.complete("finalize");

    expect(events[0].overall).toBeGreaterThan(0);
    expect(events.at(-1)!.overall).toBeCloseTo(1, 5);
    // Monotonically non-decreasing.
    for (let i = 1; i < events.length; i++) {
      expect(events[i].overall).toBeGreaterThanOrEqual(events[i - 1].overall);
    }
  });

  it("marks earlier stages complete when a later stage reports", () => {
    const events: ProgressEvent[] = [];
    const agg = new ProgressAggregator(
      ["ingest", "prepare", "probe", "finalize"],
      (e) => events.push(e),
    );
    // Jump straight to probe: ingest + prepare count as done.
    agg.update("probe", 0);
    const overallAtProbeStart = events.at(-1)!.overall;
    agg.update("ingest", 0.1); // late/out-of-order event must not regress
    expect(events.at(-1)!.overall).toBeGreaterThanOrEqual(overallAtProbeStart);
  });

  it("clamps fractions to [0, 1]", () => {
    const events: ProgressEvent[] = [];
    const agg = new ProgressAggregator(["prepare", "probe"], (e) => events.push(e));
    agg.update("prepare", 7);
    expect(events.at(-1)!.fraction).toBe(1);
    agg.update("probe", -2);
    expect(events.at(-1)!.fraction).toBe(0);
  });
});
```

Run: `cd web && npm test`
Expected: FAIL — `./progress` does not exist.

- [ ] **Step 2: Implement**

Create `web/src/processing/progress.ts`:

```ts
export type ProcessingStage = "decode" | "ingest" | "prepare" | "probe" | "finalize";

export interface ProgressEvent {
  stage: ProcessingStage;
  /** Progress within the current stage, 0..1. */
  fraction: number;
  /** Progress across the whole job, 0..1. */
  overall: number;
}

/** Rough relative cost of each stage; normalized over the stages a job runs. */
export const STAGE_WEIGHTS: Record<ProcessingStage, number> = {
  decode: 0.1,
  ingest: 0.25,
  prepare: 0.2,
  probe: 0.35,
  finalize: 0.1,
};

/**
 * Aggregates per-stage progress into a single 0..1 value. Stages run
 * sequentially: reporting on stage N marks all earlier stages complete, and
 * the overall value never decreases (late events from earlier stages are
 * absorbed).
 */
export class ProgressAggregator {
  private readonly total: number;
  private readonly fractions = new Map<ProcessingStage, number>();
  private lastOverall = 0;

  constructor(
    private readonly stages: ProcessingStage[],
    private readonly emit: (e: ProgressEvent) => void,
  ) {
    this.total = stages.reduce((sum, s) => sum + STAGE_WEIGHTS[s], 0);
  }

  update(stage: ProcessingStage, fraction: number): void {
    const idx = this.stages.indexOf(stage);
    if (idx < 0) return;
    const f = Math.min(1, Math.max(0, fraction));
    for (let i = 0; i < idx; i++) this.fractions.set(this.stages[i], 1);
    this.fractions.set(stage, Math.max(f, this.fractions.get(stage) ?? 0));

    let acc = 0;
    for (const s of this.stages) acc += STAGE_WEIGHTS[s] * (this.fractions.get(s) ?? 0);
    this.lastOverall = Math.max(this.lastOverall, acc / this.total);
    this.emit({ stage, fraction: f, overall: this.lastOverall });
  }

  complete(stage: ProcessingStage): void {
    this.update(stage, 1);
  }
}
```

Create `web/src/processing/errors.ts`:

```ts
import type { ProcessingStage } from "./progress";

/** The job was cancelled by the user. Not an error condition for the UI. */
export class CancelledError extends Error {
  constructor() {
    super("Processing cancelled");
    this.name = "CancelledError";
  }
}

/** A stage failed; `stage` tells the UI where. */
export class ProcessingError extends Error {
  constructor(
    public readonly stage: ProcessingStage,
    message: string,
  ) {
    super(message);
    this.name = "ProcessingError";
  }
}

/** Cooperative cancellation flag, checked between WASM calls. */
export class CancellationToken {
  private flag = false;

  cancel(): void {
    this.flag = true;
  }

  get cancelled(): boolean {
    return this.flag;
  }

  throwIfCancelled(): void {
    if (this.flag) throw new CancelledError();
  }
}
```

Delete `web/src/processing/setup.test.ts`.

- [ ] **Step 3: Run tests**

Run: `cd web && npm test && npx tsc -b`
Expected: progress tests PASS, type-check clean.

- [ ] **Step 4: Commit**

```bash
git add web/src/processing/progress.ts web/src/processing/errors.ts web/src/processing/progress.test.ts
git rm web/src/processing/setup.test.ts
git commit -m "feat(web): progress aggregation and cancellation primitives"
```

---### Task 14: Worker protocol — ingest messages

**Files:**
- Modify: `web/src/processing/wasm-worker.ts`
- Modify: `web/src/processing/wasm.ts`

- [ ] **Step 1: Extend the worker message types**

In `web/src/processing/wasm-worker.ts`, add to the import from `../wasm-pkg/r3sizer_wasm`: `ingest_begin, ingest_stripe, ingest_end, ingest_abort`.

Extend `WorkerRequest` (wasm-worker.ts:10-27): add `"ingest_begin" | "ingest_stripe" | "ingest_end" | "ingest_abort"` to the `type` union, and add fields:

```ts
  targetWidth?: number;
  targetHeight?: number;
  rows?: number;
```

Extend `WorkerResponse` (wasm-worker.ts:29-54): add `"ingest_result"` to the `type` union, and add:

```ts
  ingest?: { width: number; height: number } | null;
```

- [ ] **Step 2: Add the handlers**

Add to the `onmessage` dispatcher (after the `prepare_base` handler, following the same pattern as the existing blocks):

```ts
  if (msg.type === "ingest_begin") {
    const { id, width, height, targetWidth, targetHeight } = msg;
    try {
      if (!ready) throw new Error("WASM not initialized");
      const dims = ingest_begin(width!, height!, targetWidth!, targetHeight!) as {
        width: number;
        height: number;
      };
      (self as unknown as Worker).postMessage({
        type: "ingest_result",
        id,
        ingest: dims,
      } as WorkerResponse);
    } catch (err) {
      (self as unknown as Worker).postMessage({
        type: "ingest_result",
        id,
        error: err instanceof Error ? err.message : String(err),
      } as WorkerResponse);
    }
    return;
  }

  if (msg.type === "ingest_stripe") {
    const { id, rgbaData, rows } = msg;
    try {
      if (!ready) throw new Error("WASM not initialized");
      ingest_stripe(rgbaData!, rows!);
      (self as unknown as Worker).postMessage({
        type: "ingest_result",
        id,
      } as WorkerResponse);
    } catch (err) {
      (self as unknown as Worker).postMessage({
        type: "ingest_result",
        id,
        error: err instanceof Error ? err.message : String(err),
      } as WorkerResponse);
    }
    return;
  }

  if (msg.type === "ingest_end") {
    const { id } = msg;
    try {
      if (!ready) throw new Error("WASM not initialized");
      const dims = ingest_end() as { width: number; height: number };
      (self as unknown as Worker).postMessage({
        type: "ingest_result",
        id,
        ingest: dims,
      } as WorkerResponse);
    } catch (err) {
      (self as unknown as Worker).postMessage({
        type: "ingest_result",
        id,
        error: err instanceof Error ? err.message : String(err),
      } as WorkerResponse);
    }
    return;
  }

  if (msg.type === "ingest_abort") {
    // Fire-and-forget: drop partial ingest state.
    if (ready) ingest_abort();
    return;
  }
```

- [ ] **Step 3: Add client functions in `processing/wasm.ts`**

In the `onmessage` resolver chain (wasm.ts:100-116), add a branch:

```ts
            } else if (data.type === "ingest_result") {
              cb.resolve(data.ingest ?? undefined);
```

Add a transfer-capable variant next to `callWorker` (wasm.ts:346-356):

```ts
/** Like callWorker, but transfers the given buffers (zero-copy). */
function callWorkerTransfer<T>(
  msg: Omit<WorkerRequest, "id">,
  transfer: Transferable[],
): Promise<T> {
  return new Promise((resolve, reject) => {
    const id = nextId++;
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`Worker request timed out (${msg.type})`));
    }, WORKER_TIMEOUT);
    pending.set(id, { resolve, reject, timer });
    worker!.postMessage({ ...msg, id } as WorkerRequest, transfer);
  });
}
```

Add the public API (below `prepareBaseImage`):

```ts
export interface IngestDims {
  width: number;
  height: number;
}

export async function ingestBegin(
  srcW: number,
  srcH: number,
  targetW: number,
  targetH: number,
): Promise<IngestDims> {
  await ensureWorker();
  return callWorker<IngestDims>({
    type: "ingest_begin",
    width: srcW,
    height: srcH,
    targetWidth: targetW,
    targetHeight: targetH,
  });
}

export async function ingestStripe(rgba: Uint8Array, rows: number): Promise<void> {
  await ensureWorker();
  await callWorkerTransfer<IngestDims | undefined>(
    { type: "ingest_stripe", rgbaData: rgba, rows },
    [rgba.buffer as ArrayBuffer],
  );
}

export async function ingestEnd(): Promise<IngestDims> {
  await ensureWorker();
  return callWorker<IngestDims>({ type: "ingest_end" });
}

/** Fire-and-forget: drop partial WASM ingest state (cancellation path). */
export function ingestAbortFireAndForget(): void {
  worker?.postMessage({ type: "ingest_abort" } as WorkerRequest);
}

/**
 * Emergency recovery for a hung worker: terminate everything and force full
 * WASM re-initialization (with cache loss) on the next call.
 */
export function resetWorker(): void {
  if (worker) {
    worker.terminate();
    worker = null;
  }
  workerReadyPromise = null;
  for (const [id, cb] of pending) {
    clearTimeout(cb.timer);
    cb.reject(new Error("worker reset"));
    pending.delete(id);
  }
}
```

- [ ] **Step 4: Verify**

Run: `cd web && npx tsc -b && npm run lint`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add web/src/processing/wasm-worker.ts web/src/processing/wasm.ts
git commit -m "feat(web): ingest message protocol between client and WASM worker"
```

---

### Task 15: Stripe extraction, decode, and preview (`ingest.ts`)

**Files:**
- Create: `web/src/processing/ingest.ts`
- Create: `web/src/processing/ingest.test.ts`

- [ ] **Step 1: Write the failing tests (pure planning math only — canvas paths are covered by the manual validation task)**

Create `web/src/processing/ingest.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { planStripes } from "./ingest";

describe("planStripes", () => {
  it("targets ~16MB stripes for a wide panorama", () => {
    // 40000 px wide -> 160000 bytes/row -> 16MB/row-bytes = 104 rows.
    const plan = planStripes(40000, 2500);
    expect(plan.stripeHeight).toBe(104);
    expect(plan.count).toBe(Math.ceil(2500 / 104));
  });

  it("clamps stripe height to at least 16 rows", () => {
    // Absurdly wide image: 16MB / (2_000_000 * 4) = 2 rows -> clamped to 16.
    expect(planStripes(2_000_000, 100).stripeHeight).toBe(16);
  });

  it("clamps stripe height to at most 1024 rows", () => {
    // Narrow image: 16MB / (100 * 4) = 41943 rows -> clamped to 1024.
    expect(planStripes(100, 50_000).stripeHeight).toBe(1024);
  });

  it("covers every row exactly once", () => {
    const { stripeHeight, count } = planStripes(8000, 12500);
    expect((count - 1) * stripeHeight).toBeLessThan(12500);
    expect(count * stripeHeight).toBeGreaterThanOrEqual(12500);
  });
});
```

Run: `cd web && npm test`
Expected: FAIL — `./ingest` does not exist.

- [ ] **Step 2: Implement**

Create `web/src/processing/ingest.ts`:

```ts
/** Stripe extraction from a decoded ImageBitmap.
 *
 * Pixels stay in browser-managed memory (the bitmap); we extract sequential
 * full-width row stripes through one reused OffscreenCanvas, in chunks no
 * wider than 4096px (safely below all browser canvas limits), and hand each
 * stripe to the WASM worker as a transferable buffer.
 */

/** ~16MB per stripe message. */
const STRIPE_TARGET_BYTES = 16 * 1024 * 1024;
const MIN_STRIPE_ROWS = 16;
const MAX_STRIPE_ROWS = 1024;
/** Chunks of <= 4096x1024 are safely below all browser canvas limits. */
const MAX_CHUNK_WIDTH = 4096;
/** Preview shown in the UI for striped images (~2MP max). */
const PREVIEW_MAX_PIXELS = 2_000_000;

export interface StripePlan {
  stripeHeight: number;
  count: number;
}

export function planStripes(srcWidth: number, srcHeight: number): StripePlan {
  const rowBytes = srcWidth * 4;
  const stripeHeight = Math.min(
    Math.max(Math.floor(STRIPE_TARGET_BYTES / rowBytes), MIN_STRIPE_ROWS),
    MAX_STRIPE_ROWS,
  );
  return { stripeHeight, count: Math.ceil(srcHeight / stripeHeight) };
}

export function decodeToBitmap(file: File): Promise<ImageBitmap> {
  // Best-effort match with the previous loader's color behavior; perfect
  // cross-browser consistency is not claimed.
  return createImageBitmap(file, {
    premultiplyAlpha: "none",
    colorSpace: "srgb",
  });
}

/** Full-size RGBA extraction — monolithic path only (image <= threshold). */
export function bitmapToRgba(bitmap: ImageBitmap): {
  data: Uint8Array;
  width: number;
  height: number;
} {
  const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
  const ctx = canvas.getContext("2d")!;
  ctx.drawImage(bitmap, 0, 0);
  const imageData = ctx.getImageData(0, 0, bitmap.width, bitmap.height);
  return {
    data: new Uint8Array(imageData.data.buffer),
    width: bitmap.width,
    height: bitmap.height,
  };
}

/** Downscaled preview for the UI (striped path — full RGBA never exists). */
export function makePreview(bitmap: ImageBitmap): {
  rgbaData: Uint8Array;
  width: number;
  height: number;
} {
  const scale = Math.min(1, Math.sqrt(PREVIEW_MAX_PIXELS / (bitmap.width * bitmap.height)));
  const w = Math.max(1, Math.round(bitmap.width * scale));
  const h = Math.max(1, Math.round(bitmap.height * scale));
  const canvas = new OffscreenCanvas(w, h);
  const ctx = canvas.getContext("2d")!;
  ctx.drawImage(bitmap, 0, 0, w, h);
  const imageData = ctx.getImageData(0, 0, w, h);
  return { rgbaData: new Uint8Array(imageData.data.buffer), width: w, height: h };
}

/**
 * Yield sequential full-width row stripes of the bitmap as RGBA8 buffers.
 * Each stripe is assembled from <= 4096px-wide chunks through one reused
 * OffscreenCanvas.
 */
export async function* extractStripes(
  bitmap: ImageBitmap,
): AsyncGenerator<{ rgba: Uint8Array; rows: number }> {
  const { stripeHeight } = planStripes(bitmap.width, bitmap.height);
  const chunkW = Math.min(bitmap.width, MAX_CHUNK_WIDTH);
  const canvas = new OffscreenCanvas(chunkW, stripeHeight);
  const ctx = canvas.getContext("2d", { willReadFrequently: true })!;

  for (let y = 0; y < bitmap.height; y += stripeHeight) {
    const rows = Math.min(stripeHeight, bitmap.height - y);
    const stripe = new Uint8Array(bitmap.width * rows * 4);
    for (let x = 0; x < bitmap.width; x += MAX_CHUNK_WIDTH) {
      const w = Math.min(MAX_CHUNK_WIDTH, bitmap.width - x);
      ctx.clearRect(0, 0, w, rows);
      ctx.drawImage(bitmap, x, y, w, rows, 0, 0, w, rows);
      const chunk = ctx.getImageData(0, 0, w, rows).data;
      for (let r = 0; r < rows; r++) {
        stripe.set(
          chunk.subarray(r * w * 4, (r + 1) * w * 4),
          (r * bitmap.width + x) * 4,
        );
      }
    }
    yield { rgba: stripe, rows };
  }
}
```

- [ ] **Step 3: Run tests**

Run: `cd web && npm test && npx tsc -b`
Expected: PASS, clean.

- [ ] **Step 4: Commit**

```bash
git add web/src/processing/ingest.ts web/src/processing/ingest.test.ts
git commit -m "feat(web): ImageBitmap stripe extraction, decode, and preview helpers"
```

---

### Task 16: Cancellation and probe progress in the parallel pipeline

**Files:**
- Modify: `web/src/processing/probe-pool.ts`
- Modify: `web/src/processing/wasm.ts`

- [ ] **Step 1: Thread a token + chunk-progress callback through `runProbesParallel`**

In `web/src/processing/probe-pool.ts`, add the import:

```ts
import type { CancellationToken } from "./errors";
```

Change the `runProbesParallel` signature (probe-pool.ts:150) and dispatch loop:

```ts
export interface ProbeRunOpts {
  token?: CancellationToken;
  /** Called as each worker's chunk completes: (probesDone, probesTotal). */
  onChunkDone?: (done: number, total: number) => void;
}

export async function runProbesParallel(
  strengths: number[],
  paramsJson: string,
  opts: ProbeRunOpts = {},
): Promise<ProbePoolResult> {
```

and replace the `const promises = chunks.map(...)` block:

```ts
  let done = 0;
  const promises = chunks.map((chunk, i) => {
    if (chunk.length === 0) return Promise.resolve("[]");
    opts.token?.throwIfCancelled();
    return runProbeOnWorker(pool[i], chunk, paramsJson).then((json) => {
      done += chunk.length;
      opts.onChunkDone?.(done, strengths.length);
      return json;
    });
  });
```

Add a pool reset (for `client.reset()`), below `initProbePool`:

```ts
/** Terminate all probe workers; the next initProbePool call rebuilds the pool. */
export function resetPool(): void {
  for (const w of pool) w.terminate();
  pool = [];
  poolReady = null;
  workersHaveBase = false;
}
```

- [ ] **Step 2: Thread options through `processImageParallel` / `runParallelPipeline`**

In `web/src/processing/wasm.ts`, add imports:

```ts
import { CancelledError } from "./errors";
import type { CancellationToken } from "./errors";
```

Change the signatures (wasm.ts:225 and :246):

```ts
export interface ParallelOpts {
  token?: CancellationToken;
  /** Overall probing fraction 0..1 (coarse round maps to 0..0.6, dense to 0.6..1). */
  onProbeProgress?: (fraction: number) => void;
}

export async function processImageParallel(
  rgbaData: Uint8Array,
  width: number,
  height: number,
  paramsJson: string,
  opts: ParallelOpts = {},
): Promise<ProcessResult> {
  await ensureWorker();

  if (!isProbePoolReady()) {
    return processImageAsync(rgbaData, width, height, paramsJson);
  }

  try {
    return await runParallelPipeline(rgbaData, width, height, paramsJson, opts);
  } catch (err) {
    // Cancellation propagates; only real failures fall back.
    if (err instanceof CancelledError) throw err;
    return processImageAsync(rgbaData, width, height, paramsJson);
  }
}
```

In `runParallelPipeline(rgbaData, width, height, paramsJson, opts: ParallelOpts = {})`:
- After Step 1 (`prepare_base` await), Step 2 (`distributeBaseData` await), and Step 3 (strengths resolved), insert `opts.token?.throwIfCancelled();`.
- Step 4 becomes:

```ts
  const { samplesJson: initialSamplesJson } = await runProbesParallel(
    initialStrengths,
    paramsJson,
    { token: opts.token, onChunkDone: (d, t) => opts.onProbeProgress?.(0.6 * (d / t)) },
  );
```

- Before the dense round (`if (denseResult.strengths.length > 0)` body), insert `opts.token?.throwIfCancelled();`, and pass options to the dense round:

```ts
      const { samplesJson: denseSamplesJson } = await runProbesParallel(
        denseResult.strengths,
        paramsJson,
        { token: opts.token, onChunkDone: (d, t) => opts.onProbeProgress?.(0.6 + 0.4 * (d / t)) },
      );
```

- Before Step 6 (`process_from_probes`), insert `opts.token?.throwIfCancelled();` and `opts.onProbeProgress?.(1);`.

- [ ] **Step 3: Verify**

Run: `cd web && npx tsc -b && npm run lint && npm test`
Expected: clean; existing tests still pass.

- [ ] **Step 4: Commit**

```bash
git add web/src/processing/probe-pool.ts web/src/processing/wasm.ts
git commit -m "feat(web): cooperative cancellation and probe progress in parallel pipeline"
```

---

### Task 17: The processing facade (`client.ts`)

**Files:**
- Create: `web/src/processing/client.ts`
- Create: `web/src/processing/index.ts`
- Create: `web/src/processing/client.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `web/src/processing/client.test.ts`:

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AutoSharpParams } from "@/types/wasm-types";
import { DEFAULT_PARAMS } from "@/types/wasm-types";

// Mock everything below the facade.
vi.mock("./wasm", () => ({
  clearAllCaches: vi.fn(async () => {}),
  prepareImage: vi.fn(async () => {}),
  prepareBaseImage: vi.fn(async () => {}),
  processImageParallel: vi.fn(async () => ({
    imageData: new Uint8Array(4),
    outputWidth: 1,
    outputHeight: 1,
    diagnostics: {},
  })),
  ingestBegin: vi.fn(async () => ({ width: 1000, height: 750 })),
  ingestStripe: vi.fn(async () => {}),
  ingestEnd: vi.fn(async () => ({ width: 1000, height: 750 })),
  ingestAbortFireAndForget: vi.fn(),
  resetWorker: vi.fn(),
  setProgressCallback: vi.fn(),
}));
vi.mock("./probe-pool", () => ({ resetPool: vi.fn() }));
vi.mock("./ingest", () => ({
  decodeToBitmap: vi.fn(async () => fakeBitmap(8000, 6000)), // 48MP -> striped
  bitmapToRgba: vi.fn((b: ImageBitmap) => ({
    data: new Uint8Array(b.width * b.height * 4),
    width: b.width,
    height: b.height,
  })),
  makePreview: vi.fn(() => ({ rgbaData: new Uint8Array(4), width: 1, height: 1 })),
  planStripes: vi.fn(() => ({ stripeHeight: 1024, count: 3 })),
  extractStripes: vi.fn(async function* () {
    yield { rgba: new Uint8Array(8), rows: 1024 };
    yield { rgba: new Uint8Array(8), rows: 1024 };
    yield { rgba: new Uint8Array(8), rows: 952 };
  }),
}));

import * as wasm from "./wasm";
import * as ingest from "./ingest";
import { CancelledError } from "./errors";
import { ProcessingClient } from "./client";

function fakeBitmap(width: number, height: number): ImageBitmap {
  return { width, height, close: () => {} } as unknown as ImageBitmap;
}

function params(): AutoSharpParams {
  return { ...DEFAULT_PARAMS, target_width: 800, target_height: 600 };
}

describe("ProcessingClient", () => {
  let client: ProcessingClient;

  beforeEach(() => {
    client = new ProcessingClient();
    vi.clearAllMocks();
  });
  afterEach(() => vi.restoreAllMocks());

  it("classifies a >24MP image as striped and returns a preview", async () => {
    const info = await client.decode(new File([], "big.jpg"));
    expect(info.striped).toBe(true);
    expect(info.width).toBe(8000);
    expect(info.height).toBe(6000);
    expect(info.preview.rgbaData).toBeInstanceOf(Uint8Array);
  });

  it("runs the striped sequence: begin -> stripes -> end -> process", async () => {
    await client.decode(new File([], "big.jpg"));
    const result = await client.process(params()).promise;
    expect(wasm.ingestBegin).toHaveBeenCalledWith(8000, 6000, 800, 600);
    expect(wasm.ingestStripe).toHaveBeenCalledTimes(3);
    expect(wasm.ingestEnd).toHaveBeenCalledTimes(1);
    // Downstream runs on the intermediate with empty pixel data.
    expect(wasm.processImageParallel).toHaveBeenCalledWith(
      expect.objectContaining({ length: 0 }),
      1000,
      750,
      expect.any(String),
      expect.anything(),
    );
    expect(result.outputWidth).toBe(1);
  });

  it("forces uniform resize and summary diagnostics on the striped path", async () => {
    await client.decode(new File([], "big.jpg"));
    await client.process({
      ...params(),
      resize_strategy: { strategy: "uniform" } as never,
      diagnostics_level: "full",
    }).promise;
    const sentJson = vi.mocked(wasm.processImageParallel).mock.calls[0][3];
    const sent = JSON.parse(sentJson);
    expect(sent.resize_strategy).toBeNull();
    expect(sent.diagnostics_level).toBe("summary");
  });

  it("uses the monolithic path for small images", async () => {
    vi.mocked(ingest.decodeToBitmap).mockResolvedValueOnce(fakeBitmap(4000, 3000)); // 12MP
    await client.decode(new File([], "small.jpg"));
    await client.process(params()).promise;
    expect(wasm.ingestBegin).not.toHaveBeenCalled();
    expect(wasm.processImageParallel).toHaveBeenCalledWith(
      expect.objectContaining({ length: 4000 * 3000 * 4 }),
      4000,
      3000,
      expect.any(String),
      expect.anything(),
    );
  });

  it("cancel during ingest aborts and rejects with CancelledError", async () => {
    vi.mocked(wasm.ingestStripe).mockImplementation(async () => {
      job.cancel(); // cancel mid-stripe; the loop checks before the next one
    });
    await client.decode(new File([], "big.jpg"));
    const job = client.process(params());
    await expect(job.promise).rejects.toBeInstanceOf(CancelledError);
    expect(wasm.ingestAbortFireAndForget).toHaveBeenCalled();
    expect(wasm.processImageParallel).not.toHaveBeenCalled();
  });

  it("reports monotonically increasing overall progress", async () => {
    await client.decode(new File([], "big.jpg"));
    const job = client.process(params());
    const overalls: number[] = [];
    job.onProgress((e) => overalls.push(e.overall));
    await job.promise;
    expect(overalls.length).toBeGreaterThan(0);
    for (let i = 1; i < overalls.length; i++) {
      expect(overalls[i]).toBeGreaterThanOrEqual(overalls[i - 1]);
    }
    expect(overalls.at(-1)).toBeCloseTo(1, 5);
  });

  it("reset terminates workers and the probe pool", async () => {
    await client.reset();
    expect(wasm.resetWorker).toHaveBeenCalled();
  });
});
```

Run: `cd web && npm test`
Expected: FAIL — `./client` does not exist.

- [ ] **Step 2: Implement the facade**

Create `web/src/processing/client.ts`:

```ts
import type { AutoSharpParams, ProcessResult } from "@/types/wasm-types";
import { CancellationToken } from "./errors";
import {
  bitmapToRgba,
  decodeToBitmap,
  extractStripes,
  makePreview,
  planStripes,
} from "./ingest";
import { resetPool } from "./probe-pool";
import {
  clearAllCaches,
  ingestAbortFireAndForget,
  ingestBegin,
  ingestEnd,
  ingestStripe,
  prepareBaseImage,
  prepareImage,
  processImageParallel,
  resetWorker,
  setProgressCallback,
} from "./wasm";
import { ProgressAggregator, type ProcessingStage, type ProgressEvent } from "./progress";

/**
 * Images above this pixel count take the striped ingest path. This is a web
 * product policy, not a core policy: core provides the building blocks, the
 * app decides when to use them.
 */
export const DEFAULT_STRIPED_INGEST_THRESHOLD_PIXELS = 24_000_000;

export interface DecodedInput {
  /** Original source dimensions. */
  width: number;
  height: number;
  striped: boolean;
  /** Displayable pixels: full-size for monolithic, downscaled for striped. */
  preview: { rgbaData: Uint8Array; width: number; height: number };
}

export interface ProcessJob {
  readonly promise: Promise<ProcessResult>;
  onProgress(cb: (e: ProgressEvent) => void): void;
  cancel(): void;
}

interface ClientInput {
  file: File;
  bitmap: ImageBitmap;
  striped: boolean;
  /** Full-size RGBA — materialized only on the monolithic path. */
  rgba: { data: Uint8Array; width: number; height: number } | null;
}

export class ProcessingClient {
  private input: ClientInput | null = null;
  private activeJob: JobImpl | null = null;

  /**
   * Decode a file and cache it for subsequent process() calls. Decode
   * failures (e.g. Safari refusing a giant JPEG) reject here — that is the
   * spec's decode-stage error.
   */
  async decode(file: File): Promise<DecodedInput> {
    this.input?.bitmap.close();
    this.input = null;

    const bitmap = await decodeToBitmap(file);
    const striped =
      bitmap.width * bitmap.height > DEFAULT_STRIPED_INGEST_THRESHOLD_PIXELS;
    const rgba = striped ? null : bitmapToRgba(bitmap);
    this.input = { file, bitmap, striped, rgba };

    // New image: stale caches must not be reused.
    void clearAllCaches().catch(() => {});
    // Warm the linear-conversion cache (monolithic only, same as before).
    if (rgba) void prepareImage(rgba.data, rgba.width, rgba.height).catch(() => {});

    const preview = striped
      ? makePreview(bitmap)
      : { rgbaData: rgba!.data, width: rgba!.width, height: rgba!.height };
    return { width: bitmap.width, height: bitmap.height, striped, preview };
  }

  /**
   * Eagerly pre-compute the base while the user reviews params. No-op on the
   * striped path (ingest happens inside the first process() job).
   */
  prewarmBase(params: AutoSharpParams): void {
    const inp = this.input;
    if (!inp?.rgba) return;
    void prepareBaseImage(
      inp.rgba.data,
      inp.rgba.width,
      inp.rgba.height,
      JSON.stringify(params),
    ).catch(() => {});
  }

  /** Start a processing job. One active job at a time: a new job cancels the old. */
  process(params: AutoSharpParams): ProcessJob {
    if (!this.input) throw new Error("No input decoded — call decode(file) first");
    this.activeJob?.cancel();
    const job = new JobImpl(this.input, params);
    this.activeJob = job;
    void job.promise
      .catch(() => {})
      .finally(() => {
        if (this.activeJob === job) this.activeJob = null;
      });
    return job;
  }

  /**
   * Emergency recovery for a hung worker: terminate workers and probe pool;
   * the next call re-initializes WASM from scratch (cache loss).
   */
  async reset(): Promise<void> {
    this.activeJob?.cancel();
    this.activeJob = null;
    resetPool();
    resetWorker();
  }
}

class JobImpl implements ProcessJob {
  readonly promise: Promise<ProcessResult>;
  private readonly token = new CancellationToken();
  private readonly listeners: Array<(e: ProgressEvent) => void> = [];
  private readonly aggregator: ProgressAggregator;

  constructor(input: ClientInput, params: AutoSharpParams) {
    const stages: ProcessingStage[] = input.striped
      ? ["ingest", "prepare", "probe", "finalize"]
      : ["prepare", "probe", "finalize"];
    this.aggregator = new ProgressAggregator(stages, (e) => {
      for (const l of this.listeners) l(e);
    });
    this.promise = this.run(input, params);
  }

  onProgress(cb: (e: ProgressEvent) => void): void {
    this.listeners.push(cb);
  }

  cancel(): void {
    this.token.cancel();
  }

  private async run(input: ClientInput, params: AutoSharpParams): Promise<ProcessResult> {
    // Map the worker's legacy progress strings onto coarse stage events.
    setProgressCallback((stage) => {
      if (stage === "probing") this.aggregator.update("probe", 0);
      else if (stage === "fitting" || stage === "encoding")
        this.aggregator.update("finalize", 0.5);
      else this.aggregator.update("prepare", 0.5);
    });
    try {
      const result = input.striped
        ? await this.runStriped(input, params)
        : await this.runMonolithic(input, params);
      this.aggregator.complete("finalize");
      return result;
    } finally {
      setProgressCallback(null);
    }
  }

  private async runMonolithic(
    input: ClientInput,
    params: AutoSharpParams,
  ): Promise<ProcessResult> {
    this.token.throwIfCancelled();
    this.aggregator.update("prepare", 0);
    const { data, width, height } = input.rgba!;
    const result = await processImageParallel(data, width, height, JSON.stringify(params), {
      token: this.token,
      onProbeProgress: (f) => this.aggregator.update("probe", f),
    });
    this.token.throwIfCancelled();
    return result;
  }

  private async runStriped(
    input: ClientInput,
    params: AutoSharpParams,
  ): Promise<ProcessResult> {
    // Content-adaptive resize and full source-side diagnostics need the
    // whole source image, which never exists on this path.
    const effectiveParams: AutoSharpParams = {
      ...params,
      resize_strategy: null,
      diagnostics_level: "summary",
    };

    const { bitmap } = input;
    this.aggregator.update("ingest", 0);
    let inter: { width: number; height: number };
    try {
      await ingestBegin(
        bitmap.width,
        bitmap.height,
        params.target_width,
        params.target_height,
      );
      const { count } = planStripes(bitmap.width, bitmap.height);
      let sent = 0;
      for await (const { rgba, rows } of extractStripes(bitmap)) {
        this.token.throwIfCancelled();
        await ingestStripe(rgba, rows);
        sent++;
        this.aggregator.update("ingest", sent / count);
      }
      this.token.throwIfCancelled();
      inter = await ingestEnd();
    } catch (err) {
      ingestAbortFireAndForget();
      throw err;
    }

    this.aggregator.update("prepare", 0);
    // Empty pixel data: the intermediate is already the worker's cached input.
    const result = await processImageParallel(
      new Uint8Array(0),
      inter.width,
      inter.height,
      JSON.stringify(effectiveParams),
      { token: this.token, onProbeProgress: (f) => this.aggregator.update("probe", f) },
    );
    this.token.throwIfCancelled();
    return result;
  }
}

export const processingClient = new ProcessingClient();
```

Create `web/src/processing/index.ts`:

```ts
export {
  DEFAULT_STRIPED_INGEST_THRESHOLD_PIXELS,
  ProcessingClient,
  processingClient,
  type DecodedInput,
  type ProcessJob,
} from "./client";
export { CancelledError, ProcessingError } from "./errors";
export type { ProcessingStage, ProgressEvent } from "./progress";
```

- [ ] **Step 3: Run tests**

Run: `cd web && npm test && npx tsc -b && npm run lint`
Expected: all client tests PASS, type-check and lint clean.

- [ ] **Step 4: Commit**

```bash
git add web/src/processing/client.ts web/src/processing/index.ts web/src/processing/client.test.ts
git commit -m "feat(web): processing facade with striped ingest, progress, and cancellation"
```

---

### Task 18: Store refactor + upload/preview components

The store stops knowing about workers and pixel loading; components call `setInput(file)` only.

**Files:**
- Modify: `web/src/stores/processor-store.ts`
- Modify: `web/src/App.tsx`
- Modify: `web/src/components/ImageUpload.tsx`
- Modify: `web/src/components/ImagePreview.tsx`
- Delete: `web/src/lib/image-loader.ts`

- [ ] **Step 1: Refactor the store**

In `web/src/stores/processor-store.ts`:

Replace the wasm import (line 7) with:

```ts
import { CancelledError, processingClient, type ProcessJob } from "@/processing";
```

Replace the input/processing state fields in `ProcessorState`:

```ts
  // Input
  inputFile: File | null;
  /** Original source dimensions (used for aspect math and labels). */
  sourceWidth: number;
  sourceHeight: number;
  striped: boolean;
  /** Displayable pixels: full-size for monolithic, downscaled for striped. */
  previewRgbaData: Uint8Array | null;
  previewWidth: number;
  previewHeight: number;

  // Processing
  isProcessing: boolean;
  progress: { stage: string; overall: number } | null;
  error: string | null;
```

(remove `inputRgbaData`, `inputWidth`, `inputHeight`, `processingStage`), change the actions:

```ts
  setInput: (file: File) => Promise<void>;
  process: () => Promise<void>;
  cancelProcessing: () => void;
```

Initial state: `inputFile: null, sourceWidth: 0, sourceHeight: 0, striped: false, previewRgbaData: null, previewWidth: 0, previewHeight: 0, isProcessing: false, progress: null, error: null` (replacing the old input/processing defaults).

New `setInput` (replaces processor-store.ts:161-213; keeps the orientation/aspect logic verbatim, swapping its inputs):

```ts
  setInput: async (file) => {
    let decoded;
    try {
      decoded = await processingClient.decode(file);
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e) });
      return;
    }
    const { width, height } = decoded;

    const state = get();
    const params = { ...state.params };
    const isPortrait = height > width;

    if (!state.lockDimensions) {
      const saved = loadDimsForOrientation(isPortrait);
      params.target_width = saved.width;
      params.target_height = saved.height;

      if (state.preserveAspectRatio) {
        const aspect = width / height;
        if (isPortrait) {
          params.target_width = Math.round(params.target_height * aspect);
        } else {
          params.target_height = Math.round(params.target_width / aspect);
        }
      }

      saveDims(isPortrait, params.target_width, params.target_height);
    }

    set({
      inputFile: file,
      sourceWidth: width,
      sourceHeight: height,
      striped: decoded.striped,
      previewRgbaData: decoded.preview.rgbaData,
      previewWidth: decoded.preview.width,
      previewHeight: decoded.preview.height,
      params,
      outputRgbaData: null,
      outputWidth: 0,
      outputHeight: 0,
      diagnostics: null,
      error: null,
    });

    // Eagerly pre-compute the base while the user reviews params
    // (monolithic only — the facade no-ops for striped inputs).
    processingClient.prewarmBase(params);
  },
```

New `process` + `cancelProcessing` (replaces processor-store.ts:270-310; keep a module-level `let currentJob: ProcessJob | null = null;` above the store creation — a job handle is not UI state):

```ts
  process: async () => {
    const state = get();
    if (!state.inputFile) {
      set({ error: "No image loaded" });
      return;
    }

    set({ isProcessing: true, progress: null, error: null });

    try {
      const job = processingClient.process(state.params);
      currentJob = job;
      job.onProgress(({ stage, overall }) => set({ progress: { stage, overall } }));
      const result = await job.promise;

      set({
        outputRgbaData: result.imageData,
        outputWidth: result.outputWidth,
        outputHeight: result.outputHeight,
        diagnostics: result.diagnostics,
        lastProcessedParams: { ...state.params },
        lastProcessedVersion: get().paramsVersion,
        isProcessing: false,
        progress: null,
      });
    } catch (e) {
      if (e instanceof CancelledError) {
        set({ isProcessing: false, progress: null });
      } else {
        set({
          error: e instanceof Error ? e.message : String(e),
          isProcessing: false,
          progress: null,
        });
      }
    } finally {
      currentJob = null;
    }
  },

  cancelProcessing: () => {
    currentJob?.cancel();
  },
```

Update `reset` to clear the new field names.

- [ ] **Step 2: Update components**

`web/src/components/ImageUpload.tsx` (line 17) and `web/src/App.tsx` (line 45): replace

```ts
      loadImageAsRgba(file).then(({ data, width, height }) => {
        setInput(file, data, width, height);
      });
```

with

```ts
      void setInput(file);
```

and delete the `loadImageAsRgba` imports (App.tsx:14, ImageUpload.tsx:5).

`web/src/components/ImagePreview.tsx` (lines 195-240): switch the selectors —

```ts
  const previewRgbaData = useProcessorStore((s) => s.previewRgbaData);
  const previewWidth = useProcessorStore((s) => s.previewWidth);
  const previewHeight = useProcessorStore((s) => s.previewHeight);
  const sourceWidth = useProcessorStore((s) => s.sourceWidth);
  const sourceHeight = useProcessorStore((s) => s.sourceHeight);
```

- `if (!inputRgbaData) return null;` → `if (!previewRgbaData) return null;`
- The `ComparisonSlider` props: `inputRgba={previewRgbaData} inputW={previewWidth} inputH={previewHeight}` (the slider renders pixels — they must match the buffer's real dimensions).
- The dimension *labels* (`{inputWidth}×{inputHeight}`) → `{sourceWidth}×{sourceHeight}` (the user should see source dimensions, not preview dimensions).
- `<FittedCanvas rgbaData={previewRgbaData} width={previewWidth} height={previewHeight} />`.

Delete `web/src/lib/image-loader.ts` and verify nothing else imports it: `grep -rn "image-loader" web/src` must return nothing.

- [ ] **Step 3: Verify**

Run: `cd web && npx tsc -b && npm run lint && npm test`
Expected: clean. (App.tsx still references `processingStage` for the overlay — Task 19 fixes that; if `tsc` flags it now, apply the Task 19 App.tsx change in the same commit and note it.)

`tsc` will fail on `ProcessingOverlay stage={processingStage}` — do Task 19's App.tsx + overlay changes together with this task's commit if needed, or temporarily pass `stage={progress?.stage ?? null}` to the unmodified overlay (it accepts `string | null`). Use the temporary bridge; Task 19 replaces it.

- [ ] **Step 4: Commit**

```bash
git add -A web/src
git commit -m "refactor(web): store and components consume the processing facade"
```

---

### Task 19: ProcessingOverlay with progress bar and cancel

**Files:**
- Modify: `web/src/components/ProcessingOverlay.tsx`
- Modify: `web/src/App.tsx` (overlay props)
- Modify: `web/public/locales/en.json`, `web/public/locales/ru.json`

- [ ] **Step 1: Rewrite the overlay**

Replace the component in `web/src/components/ProcessingOverlay.tsx` (keep the file's existing imports for `Loader2`, `useTranslation`, `AnimatePresence`/`motion`):

```tsx
export function ProcessingOverlay({
  stage,
  overall,
  onCancel,
}: {
  stage: string | null;
  overall: number;
  onCancel: () => void;
}) {
  const { t } = useTranslation();

  return (
    <div className="absolute inset-0 z-20 flex items-center justify-center bg-background/60 backdrop-blur-[2px]">
      <div className="flex w-56 flex-col items-center gap-2.5">
        <Loader2 className="h-6 w-6 animate-spin text-primary" />
        <AnimatePresence mode="popLayout">
          <motion.span
            key={stage}
            initial={{ opacity: 0, filter: "blur(4px)", y: 4 }}
            animate={{ opacity: 1, filter: "blur(0px)", y: 0 }}
            exit={{ opacity: 0, filter: "blur(4px)", y: -4 }}
            transition={{ duration: 0.15, ease: "easeOut" }}
            className="text-sm font-mono text-primary/80 tracking-wide"
          >
            {stage ? t(`processing.stages.${stage}`) : t("processing.starting")}
          </motion.span>
        </AnimatePresence>
        <div className="h-1 w-full overflow-hidden rounded-full bg-border/40">
          <div
            className="h-full bg-primary transition-[width] duration-200"
            style={{ width: `${Math.round(overall * 100)}%` }}
          />
        </div>
        <button
          type="button"
          onClick={onCancel}
          className="mt-1 text-[11px] font-mono uppercase tracking-widest text-muted-foreground/60 transition-colors hover:text-foreground"
        >
          {t("processing.cancel")}
        </button>
      </div>
    </div>
  );
}
```

In `web/src/App.tsx`, the overlay usage (currently `{isProcessing && <ProcessingOverlay stage={processingStage} />}` around line 119) becomes — adding `progress` and `cancelProcessing` to the store selectors App already uses:

```tsx
      {isProcessing && (
        <ProcessingOverlay
          stage={progress?.stage ?? null}
          overall={progress?.overall ?? 0}
          onCancel={cancelProcessing}
        />
      )}
```

- [ ] **Step 2: Add i18n keys**

In `web/public/locales/en.json`, the `"processing"` object (line ~157) becomes:

```json
  "processing": {
    "starting": "starting...",
    "cancel": "Cancel",
    "stages": {
      "decode": "decoding...",
      "ingest": "streaming image...",
      "prepare": "preparing...",
      "probe": "probing...",
      "finalize": "finalizing..."
    }
  }
```

In `web/public/locales/ru.json`, the matching object:

```json
  "processing": {
    "starting": "запуск...",
    "cancel": "Отмена",
    "stages": {
      "decode": "декодирование...",
      "ingest": "потоковая загрузка...",
      "prepare": "подготовка...",
      "probe": "пробы...",
      "finalize": "финализация..."
    }
  }
```

(Preserve any other existing keys inside `"processing"` in both files.)

- [ ] **Step 3: Verify**

Run: `cd web && npx tsc -b && npm run lint`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add web/src/components/ProcessingOverlay.tsx web/src/App.tsx web/public/locales/en.json web/public/locales/ru.json
git commit -m "feat(web): progress bar and cancel button in processing overlay"
```

---

### Task 20: Striped-ingest badge in diagnostics

**Files:**
- Modify: `web/src/components/diagnostics/SummaryTab.tsx`
- Modify: `web/public/locales/en.json`, `web/public/locales/ru.json`

- [ ] **Step 1: Add the readout**

In `web/src/components/diagnostics/SummaryTab.tsx`, directly after the input-size `Readout` (the one rendering `diagnostics.input_size.width × height`, ~line 197), add:

```tsx
        {diagnostics.ingest?.striped && (
          <Readout
            label={t("diagnostics.stripedIngest")}
            value={`${diagnostics.ingest.original_width}×${diagnostics.ingest.original_height}`}
          />
        )}
```

Note: on the striped path `input_size` is the *intermediate*; this readout shows the true source size. Match the surrounding `Readout` usage exactly (if the existing ones pass extra props like `mono`, mirror them).

- [ ] **Step 2: i18n keys**

Add inside the `"diagnostics"` object of `en.json`:

```json
    "stripedIngest": "striped ingest (source)",
```

and `ru.json`:

```json
    "stripedIngest": "потоковый ввод (исходник)",
```

- [ ] **Step 3: Verify and commit**

Run: `cd web && npx tsc -b && npm run lint`

```bash
git add web/src/components/diagnostics/SummaryTab.tsx web/public/locales/en.json web/public/locales/ru.json
git commit -m "feat(web): striped-ingest source-size badge in diagnostics summary"
```

---

### Task 21: Full verification + manual large-image validation

**Files:**
- Create: `web/tools/generate-large-image.html`

- [ ] **Step 1: Run every automated gate**

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
cd web && npm test && npm run lint && npm run build
```

Expected: everything green. `npm run build` includes the wasm-pack build and `tsc -b`.

- [ ] **Step 2: Create the test-image generator**

Create `web/tools/generate-large-image.html`:

```html
<!doctype html>
<meta charset="utf-8" />
<title>Generate 100MP test image</title>
<p>Generates a 16000×6250 (100MP) PNG with gradient + speckle detail so the
probing pipeline has artifacts to measure. Open this file directly in a
browser and click the button.</p>
<button id="go">Generate panorama-100mp.png</button>
<script>
  document.getElementById("go").onclick = async () => {
    const W = 16000, H = 6250;
    const canvas = new OffscreenCanvas(W, H);
    const ctx = canvas.getContext("2d");
    const grad = ctx.createLinearGradient(0, 0, W, 0);
    grad.addColorStop(0, "#0a2a4a");
    grad.addColorStop(1, "#e8c87a");
    ctx.fillStyle = grad;
    ctx.fillRect(0, 0, W, H);
    ctx.fillStyle = "rgba(255,255,255,0.35)";
    for (let i = 0; i < 250000; i++) {
      ctx.fillRect(Math.random() * W, Math.random() * H, 2, 2);
    }
    const blob = await canvas.convertToBlob({ type: "image/png" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = "panorama-100mp.png";
    a.click();
  };
</script>
```

- [ ] **Step 3: Manual validation checklist (run `cd web && npm run dev`, Chrome)**

1. Generate `panorama-100mp.png` with the tool page; load it in the app.
2. The preview renders (downscaled), labels show 16000×6250.
3. Process with an 800-wide target: progress bar advances through *streaming image → preparing → probing → finalizing*; result appears.
4. DevTools → Memory: WASM heap stays under ~150 MB during the whole run.
5. Click Cancel mid-ingest and mid-probe: UI returns to idle in < 1 s, no error toast, a subsequent process run succeeds.
6. Diagnostics panel shows the striped-ingest badge with `16000×6250`.
7. Load a small (< 24MP) image: behavior identical to before this feature (preview, processing, diagnostics, no badge).
8. Striped + content-adaptive resize selected in params: processing succeeds (uniform forced), no crash.
9. Large image with a target close to source size (e.g. 12000 wide): a clear error message appears ("target too close to source..." mapped text).

- [ ] **Step 4: Commit**

```bash
git add web/tools/generate-large-image.html
git commit -m "chore(web): 100MP test-image generator for manual validation"
```

---

## Self-Review (done while writing — verified)

- **Spec coverage:** §5 core API → Tasks 1-7; ratio error → Task 2; diagnostics type/threading → Task 8; §6 WASM exports → Tasks 10, 14; §7 facade/ingest/progress/cancel → Tasks 13-17; threshold constant → Task 17; store/UI → Tasks 18-19; badge → Task 20; §8 error mapping → Tasks 2, 10, 17 (decode rejection in `decode()`, abort-on-error in `runStriped`), reset/timeout recovery → Tasks 14, 17 (existing 30s per-call timeouts + `reset()`); §9 testing → Tasks 5-7, 9, 11, 17; §10 priority order preserved.
- **Known simplifications (intentional):** per-stage *watchdog → auto-reset* is not wired automatically — timeouts reject the job and `client.reset()` is exposed for recovery (the spec's "emergency recovery" path); the spec's `process(file, params)` is split into `decode(file)` + `process(params)` (documented in the header); `decode` stage progress happens at upload time.
- **Type consistency check:** `StripedPreReducer::new(ImageSize, ImageSize)`, `push_srgb8_rows(&[u8], u32)`, `finish() -> Result<LinearRgbImage, CoreError>` used identically in Tasks 5, 9, 10. `IngestDims {width, height}` consistent across Tasks 14, 17. Store field names (`previewRgbaData`, `sourceWidth`) consistent across Tasks 18-19.
