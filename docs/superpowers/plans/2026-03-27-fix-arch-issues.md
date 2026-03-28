# Architectural Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the six architectural issues identified in the 2026-03-27 review: stray dependency, misleading error variant, dead enum variant, misplaced helpers, allocation waste in probe loop, and monolithic types file.

**Architecture:** All changes are confined to `r3sizer-core` and `r3sizer-io`. No public pipeline API signature changes. Tasks are ordered from smallest blast radius to largest.

**Tech Stack:** Rust 2021, `thiserror`, `serde`, `image` crate. Test framework: built-in + `approx`. Build: `cargo test --workspace && cargo clippy --workspace -- -D warnings`.

---

## Task 1: Remove `serde_json` from `r3sizer-io`

`r3sizer-io` lists `serde_json` as a dependency but nothing in that crate uses JSON. JSON serialization happens in `r3sizer-cli`.

**Files:**
- Modify: `crates/r3sizer-io/Cargo.toml`

- [ ] **Step 1: Verify `serde_json` is unused in `r3sizer-io`**

```bash
grep -r "serde_json" crates/r3sizer-io/src/
```
Expected output: no matches.

- [ ] **Step 2: Remove the dependency**

In `crates/r3sizer-io/Cargo.toml`, remove line 9:
```toml
serde_json    = { workspace = true }
```

Final `[dependencies]` section should read:
```toml
[dependencies]
thiserror     = { workspace = true }
image         = { workspace = true }
r3sizer-core = { path = "../r3sizer-core" }
```

- [ ] **Step 3: Verify it still compiles**

```bash
cargo build --workspace
```
Expected: no errors.

- [ ] **Step 4: Run tests**

```bash
cargo test --workspace
```
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/r3sizer-io/Cargo.toml
git commit -m "chore: remove unused serde_json dep from r3sizer-io"
```

---

## Task 2: Document `CoreError::NoValidRoot` — clarify when it is returned

`NoValidRoot` is only reachable when `solve::find_sharpness` or `solve::find_sharpness_direct` is called with an empty `probe_samples` slice. Through `process_auto_sharp_downscale`, this cannot happen because `params.validate()` enforces at least 4 probe strengths. The current doc-comment on `process_auto_sharp_downscale` says "in pathological cases" without explaining what those cases are.

**Files:**
- Modify: `crates/r3sizer-core/src/lib.rs` (error variant doc comment)
- Modify: `crates/r3sizer-core/src/solve.rs` (doc on `fallback_from_samples`)
- Modify: `crates/r3sizer-core/src/pipeline.rs` (function-level doc)

- [ ] **Step 1: Add a doc comment to `CoreError::NoValidRoot`**

In `crates/r3sizer-core/src/lib.rs`, replace:
```rust
    #[error("no valid sharpening root found: {reason}")]
    NoValidRoot { reason: String },
```
with:
```rust
    /// Returned by [`solve::find_sharpness`] and [`solve::find_sharpness_direct`]
    /// when called with an empty `probe_samples` slice.
    ///
    /// This variant is **unreachable through the main pipeline** (`process_auto_sharp_downscale`)
    /// because parameter validation guarantees at least 4 probe samples before the solver
    /// is invoked.  It can only occur if you call the `solve` functions directly with
    /// an empty slice.
    #[error("no valid sharpening root found: {reason}")]
    NoValidRoot { reason: String },
```

- [ ] **Step 2: Add a note to `fallback_from_samples` in `solve.rs`**

In `crates/r3sizer-core/src/solve.rs`, replace the opening of `fallback_from_samples`:
```rust
fn fallback_from_samples(
    samples: &[ProbeSample],
    p0: f32,
) -> Result<SolveResult, CoreError> {
    if samples.is_empty() {
        return Err(CoreError::NoValidRoot {
            reason: "no probe samples available for fallback selection".into(),
        });
    }
```
with:
```rust
fn fallback_from_samples(
    samples: &[ProbeSample],
    p0: f32,
) -> Result<SolveResult, CoreError> {
    // This branch is unreachable through the pipeline: `params.validate()` enforces
    // at least 4 probe strengths, so `samples` is always non-empty here.
    // The error is only reachable if `find_sharpness_direct` is called directly
    // with an empty slice.
    if samples.is_empty() {
        return Err(CoreError::NoValidRoot {
            reason: "no probe samples available for fallback selection".into(),
        });
    }
```

- [ ] **Step 3: Tighten the pipeline doc comment**

In `crates/r3sizer-core/src/pipeline.rs`, replace:
```rust
/// # Errors
///
/// Returns `Err` for invalid parameters or (in pathological cases) when both
/// the cubic-root path and the probe-sample fallback have nothing to offer.
```
with:
```rust
/// # Errors
///
/// Returns `Err(CoreError::InvalidParams)` if `params.validate()` fails (e.g.
/// zero dimensions, negative sigma, or fewer than 4 probe strengths).
/// All other error variants from this function indicate truly unexpected
/// numerical failures (singular fit matrix with degenerate probe data).
/// `CoreError::NoValidRoot` is **not** returned by this function because
/// parameter validation guarantees a non-empty probe sample list.
```

- [ ] **Step 4: Build and test**

```bash
cargo test --workspace && cargo clippy --workspace -- -D warnings
```
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/r3sizer-core/src/lib.rs crates/r3sizer-core/src/solve.rs crates/r3sizer-core/src/pipeline.rs
git commit -m "docs: clarify CoreError::NoValidRoot reachability"
```

---

## Task 3: Remove `FitStrategy::ForcedLinear`

`ForcedLinear` is documented as "Force a linear (degree-1) fit" but is matched together with `Cubic` in `pipeline.rs` and calls `fit_cubic`. It has no distinct behaviour. No test exercises any `ForcedLinear`-specific path.

**Files:**
- Modify: `crates/r3sizer-core/src/types.rs`
- Modify: `crates/r3sizer-core/src/pipeline.rs`

- [ ] **Step 1: Remove the variant from `FitStrategy`**

In `crates/r3sizer-core/src/types.rs`, replace:
```rust
/// Polynomial fit strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FitStrategy {
    /// Least-squares cubic fit; fall back to direct sampled search if numerically unstable.
    Cubic,
    /// Skip fitting; pick best strength directly from probe samples.
    DirectSearch,
    /// Force a linear (degree-1) fit. Useful for diagnostics.
    ForcedLinear,
}
```
with:
```rust
/// Polynomial fit strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FitStrategy {
    /// Least-squares cubic fit; fall back to direct sampled search if numerically unstable.
    Cubic,
    /// Skip fitting; pick best strength directly from probe samples.
    DirectSearch,
}
```

- [ ] **Step 2: Fix the match in `pipeline.rs`**

In `crates/r3sizer-core/src/pipeline.rs`, replace:
```rust
        FitStrategy::ForcedLinear | FitStrategy::Cubic => {
```
with:
```rust
        FitStrategy::Cubic => {
```

- [ ] **Step 3: Verify no other references**

```bash
grep -r "ForcedLinear" .
```
Expected: no matches.

- [ ] **Step 4: Build and test**

```bash
cargo test --workspace && cargo clippy --workspace -- -D warnings
```
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/r3sizer-core/src/types.rs crates/r3sizer-core/src/pipeline.rs
git commit -m "refactor: remove FitStrategy::ForcedLinear (unimplemented, same as Cubic)"
```

---

## Task 4: Move `to_linear_inplace` / `to_srgb_inplace` out of `pipeline.rs`

These are color-conversion helpers that live in `pipeline.rs` but belong in `color.rs`. They wrap `color::image_srgb_to_linear` and `color::image_linear_to_srgb` with no added logic.

**Files:**
- Modify: `crates/r3sizer-core/src/color.rs`
- Modify: `crates/r3sizer-core/src/pipeline.rs`
- Modify: `crates/r3sizer-core/src/lib.rs`

- [ ] **Step 1: Add the public functions to `color.rs`**

At the end of `crates/r3sizer-core/src/color.rs`, after `image_linear_to_srgb` and before the `#[cfg(test)]` block, add:

```rust
/// Convenience alias: convert an sRGB-encoded `LinearRgbImage` to linear light in place.
///
/// Equivalent to calling [`image_srgb_to_linear`].
pub fn to_linear_inplace(img: &mut LinearRgbImage) {
    image_srgb_to_linear(img);
}

/// Convenience alias: convert a linear `LinearRgbImage` to sRGB-encoded in place.
///
/// Equivalent to calling [`image_linear_to_srgb`].
pub fn to_srgb_inplace(img: &mut LinearRgbImage) {
    image_linear_to_srgb(img);
}
```

- [ ] **Step 2: Remove the functions from `pipeline.rs`**

In `crates/r3sizer-core/src/pipeline.rs`, delete the entire section from line 271 to the end of the file (the block starting with `// Convert sRGB-encoded...`):

```rust
// ---------------------------------------------------------------------------
// Convert sRGB-encoded LinearRgbImage to/from linear for pipeline callers
// who manage colour conversion outside the pipeline.
// ---------------------------------------------------------------------------

/// Convenience: convert an sRGB-encoded image (loaded from a file) to linear
/// RGB in place, ready for the pipeline.
pub fn to_linear_inplace(img: &mut LinearRgbImage) {
    color::image_srgb_to_linear(img);
}

/// Convenience: convert a linear RGB image back to sRGB in place, ready for
/// file encoding.
pub fn to_srgb_inplace(img: &mut LinearRgbImage) {
    color::image_linear_to_srgb(img);
}
```

- [ ] **Step 3: Update `lib.rs` re-exports**

In `crates/r3sizer-core/src/lib.rs`, add re-exports for the moved functions. After the existing `pub use pipeline::process_auto_sharp_downscale;` line, add:

```rust
pub use color::{to_linear_inplace, to_srgb_inplace};
```

- [ ] **Step 4: Build and test**

```bash
cargo test --workspace && cargo clippy --workspace -- -D warnings
```
Expected: all pass. The public API surface is unchanged (same names, same signatures).

- [ ] **Step 5: Commit**

```bash
git add crates/r3sizer-core/src/color.rs crates/r3sizer-core/src/pipeline.rs crates/r3sizer-core/src/lib.rs
git commit -m "refactor: move to_linear_inplace/to_srgb_inplace from pipeline to color"
```

---

## Task 5: Pre-allocate probe scratch buffer

Each probe iteration calls `sharpen_image(...)` which returns a fresh `LinearRgbImage` allocation (~6 MB at 960×540). With 7 probes that's 7 allocations; only the measurement matters, not the storage lifetime. Adding `_inplace` variants to the sharpening functions reduces this to a single scratch allocation that is overwritten each iteration.

**Files:**
- Modify: `crates/r3sizer-core/src/sharpen.rs` (add `_inplace` variants)
- Modify: `crates/r3sizer-core/src/color.rs` (add `reconstruct_rgb_from_lightness_inplace`)
- Modify: `crates/r3sizer-core/src/pipeline.rs` (pre-allocate scratch, use `_inplace` in loop)

- [ ] **Step 1: Add `unsharp_mask_inplace` to `sharpen.rs`**

In `crates/r3sizer-core/src/sharpen.rs`, after `unsharp_mask_single_channel` and before `#[cfg(test)]`, add:

```rust
/// Apply unsharp-mask to `src`, writing the result into `out`.
///
/// `out` must have the same dimensions as `src`. Panics in debug builds if
/// dimensions differ. This variant avoids one heap allocation per call versus
/// [`unsharp_mask`] by reusing the caller-supplied output buffer.
///
/// The result is **not clamped**.
pub fn unsharp_mask_inplace(
    src: &LinearRgbImage,
    out: &mut LinearRgbImage,
    amount: f32,
    sigma: f32,
) -> Result<(), CoreError> {
    if sigma <= 0.0 {
        return Err(CoreError::InvalidParams("sharpen_sigma must be positive".into()));
    }
    debug_assert_eq!(src.width(), out.width());
    debug_assert_eq!(src.height(), out.height());

    let kernel = gaussian_kernel(sigma);
    let blurred = gaussian_blur(src, &kernel);

    let out_pixels = out.pixels_mut();
    for (i, (&s, &b)) in src.pixels().iter().zip(blurred.pixels().iter()).enumerate() {
        out_pixels[i] = s + amount * (s - b);
    }
    Ok(())
}

/// Apply single-channel unsharp-mask, writing the result into `out`.
///
/// `out` must have length `width * height`. Panics in debug builds if lengths
/// differ. Avoids one heap allocation per call versus
/// [`unsharp_mask_single_channel`].
///
/// The result is **not clamped**.
pub fn unsharp_mask_single_channel_inplace(
    data: &[f32],
    out: &mut [f32],
    width: usize,
    height: usize,
    amount: f32,
    sigma: f32,
) -> Result<(), CoreError> {
    if sigma <= 0.0 {
        return Err(CoreError::InvalidParams("sharpen_sigma must be positive".into()));
    }
    debug_assert_eq!(data.len(), width * height);
    debug_assert_eq!(out.len(), width * height);

    let kernel = gaussian_kernel(sigma);
    let blurred = gaussian_blur_single_channel(data, width, height, &kernel);

    for (i, (&s, &b)) in data.iter().zip(blurred.iter()).enumerate() {
        out[i] = s + amount * (s - b);
    }
    Ok(())
}
```

- [ ] **Step 2: Add `reconstruct_rgb_from_lightness_inplace` to `color.rs`**

In `crates/r3sizer-core/src/color.rs`, after `reconstruct_rgb_from_lightness` and before `image_srgb_to_linear`, add:

```rust
/// Reconstruct RGB from original linear RGB and sharpened luminance, writing into `out`.
///
/// `out` must have the same dimensions as `original`. This variant avoids one
/// heap allocation per call versus [`reconstruct_rgb_from_lightness`].
///
/// See [`reconstruct_rgb_from_lightness`] for the reconstruction formula and
/// engineering-approximation note.
pub fn reconstruct_rgb_from_lightness_inplace(
    original: &LinearRgbImage,
    sharpened_luminance: &[f32],
    out: &mut LinearRgbImage,
) {
    const EPSILON: f32 = 1e-6;
    debug_assert_eq!(
        sharpened_luminance.len(),
        (original.width() as usize) * (original.height() as usize),
    );
    debug_assert_eq!(original.width(), out.width());
    debug_assert_eq!(original.height(), out.height());

    let out_pixels = out.pixels_mut();
    for (i, (rgb, &l_sharp)) in original
        .pixels()
        .chunks_exact(3)
        .zip(sharpened_luminance.iter())
        .enumerate()
    {
        let (r, g, b) = (rgb[0], rgb[1], rgb[2]);
        let l_orig = luminance_from_linear_srgb(r, g, b);
        let (ro, go, bo) = if l_orig.abs() < EPSILON {
            (r, g, b)
        } else {
            let k = l_sharp / l_orig;
            (k * r, k * g, k * b)
        };
        out_pixels[i * 3] = ro;
        out_pixels[i * 3 + 1] = go;
        out_pixels[i * 3 + 2] = bo;
    }
}
```

- [ ] **Step 3: Add a `sharpen_image_inplace` helper to `pipeline.rs`**

In `crates/r3sizer-core/src/pipeline.rs`, update the imports at the top to include the new inplace functions:

```rust
use crate::{
    color,
    contrast::{apply_contrast_leveling, ContrastLevelingParams},
    fit::fit_cubic,
    metrics::artifact_ratio,
    resize::downscale,
    sharpen::{unsharp_mask, unsharp_mask_inplace, unsharp_mask_single_channel,
              unsharp_mask_single_channel_inplace},
    solve::{find_sharpness, find_sharpness_direct},
    AutoSharpDiagnostics, AutoSharpParams, ClampPolicy, FitStatus,
    FitStrategy, ImageSize, LinearRgbImage, MetricMode, ProbeSample, ProcessOutput,
    SelectionMode, SharpenMode, CoreError,
};
```

Then add this helper after the existing `sharpen_image` function:

```rust
/// Apply sharpening in-place into `out`. `out` must be the same size as `base`.
/// Avoids one `LinearRgbImage` allocation per probe iteration.
fn sharpen_image_inplace(
    base: &LinearRgbImage,
    base_luminance: Option<&[f32]>,
    sharpened_lum_scratch: &mut Vec<f32>,
    mode: SharpenMode,
    amount: f32,
    sigma: f32,
    out: &mut LinearRgbImage,
) -> Result<(), CoreError> {
    match mode {
        SharpenMode::Rgb => unsharp_mask_inplace(base, out, amount, sigma),
        SharpenMode::Lightness => {
            let lum = base_luminance
                .expect("base_luminance must be provided for Lightness mode");
            let w = base.width() as usize;
            let h = base.height() as usize;
            unsharp_mask_single_channel_inplace(
                lum,
                sharpened_lum_scratch,
                w,
                h,
                amount,
                sigma,
            )?;
            color::reconstruct_rgb_from_lightness_inplace(
                base,
                sharpened_lum_scratch,
                out,
            );
            Ok(())
        }
    }
}
```

- [ ] **Step 4: Pre-allocate scratch buffers and use `_inplace` in the probe loop**

In `crates/r3sizer-core/src/pipeline.rs`, replace the probe-loop section (from the `let mut probe_samples` line through the closing `}` of the `for &s in &strengths` loop) with:

```rust
    let mut probe_samples: Vec<ProbeSample> = Vec::with_capacity(strengths.len());

    // Pre-allocate a single scratch image reused across all probe iterations
    // to avoid N separate heap allocations (one per probe).
    let mut probe_scratch = LinearRgbImage::zeros(target.width, target.height)?;

    // Pre-allocate sharpened-luminance scratch for Lightness mode (single channel).
    let pixel_count = (target.width as usize) * (target.height as usize);
    let mut sharpened_lum_scratch: Vec<f32> = if matches!(params.sharpen_mode, SharpenMode::Lightness) {
        vec![0.0f32; pixel_count]
    } else {
        Vec::new()
    };

    for &s in &strengths {
        sharpen_image_inplace(
            &base,
            base_luminance.as_deref(),
            &mut sharpened_lum_scratch,
            params.sharpen_mode,
            s,
            sigma,
            &mut probe_scratch,
        )?;
        let p_total = artifact_ratio(&probe_scratch);
        let metric_value = compute_metric_value(
            p_total,
            baseline_artifact_ratio,
            params.metric_mode,
        );
        probe_samples.push(ProbeSample {
            strength: s,
            artifact_ratio: p_total,
            metric_value,
        });
    }
```

- [ ] **Step 5: Build and test**

```bash
cargo test --workspace && cargo clippy --workspace -- -D warnings
```
Expected: all pass. The `sharpen_image` (allocating) function is still used by the final sharpening apply (step 8 of the pipeline), which only runs once.

- [ ] **Step 6: Commit**

```bash
git add crates/r3sizer-core/src/sharpen.rs crates/r3sizer-core/src/color.rs crates/r3sizer-core/src/pipeline.rs
git commit -m "perf: pre-allocate probe scratch buffer to avoid N heap allocs per pipeline run"
```

---

## Task 6: Split `types.rs` into focused submodules

`types.rs` is 382 lines containing four unrelated domains: the image buffer, configuration parameters, status enums, and result types. Each becomes its own file under a `types/` subdirectory. The re-export surface in `types/mod.rs` keeps the public API identical — no other files change.

**Files to create:**
- `crates/r3sizer-core/src/types/image.rs`
- `crates/r3sizer-core/src/types/params.rs`
- `crates/r3sizer-core/src/types/status.rs`
- `crates/r3sizer-core/src/types/results.rs`
- `crates/r3sizer-core/src/types/mod.rs` (replaces `types.rs`)

**Files to delete:**
- `crates/r3sizer-core/src/types.rs`

- [ ] **Step 1: Create `types/image.rs`**

Create file `crates/r3sizer-core/src/types/image.rs` with this exact content:

```rust
use serde::{Deserialize, Serialize};

use crate::CoreError;

/// Owned linear-RGB image buffer.
///
/// Pixel layout: interleaved `[R, G, B, R, G, B, …]` in row-major order.
/// Values are nominally in `[0.0, 1.0]` but intermediate processing stages
/// intentionally allow values outside that range (e.g. after sharpening).
/// Clamping to the valid range happens only at the final output stage.
#[derive(Debug, Clone)]
pub struct LinearRgbImage {
    width: u32,
    height: u32,
    /// Length == width * height * 3.
    data: Vec<f32>,
}

impl LinearRgbImage {
    /// Create a new image. Returns an error if `data.len() != width * height * 3`
    /// or if either dimension is zero.
    pub fn new(width: u32, height: u32, data: Vec<f32>) -> Result<Self, CoreError> {
        if width == 0 || height == 0 {
            return Err(CoreError::EmptyImage);
        }
        let expected = (width as usize) * (height as usize) * 3;
        if data.len() != expected {
            return Err(CoreError::BufferLengthMismatch {
                expected_len: expected,
                got_len: data.len(),
            });
        }
        Ok(Self { width, height, data })
    }

    /// Create an all-zero (black) image of the given size.
    pub fn zeros(width: u32, height: u32) -> Result<Self, CoreError> {
        if width == 0 || height == 0 {
            return Err(CoreError::EmptyImage);
        }
        let len = (width as usize) * (height as usize) * 3;
        Ok(Self { width, height, data: vec![0.0f32; len] })
    }

    pub fn width(&self) -> u32 { self.width }
    pub fn height(&self) -> u32 { self.height }

    /// Read-only flat slice of all pixel components.
    pub fn pixels(&self) -> &[f32] { &self.data }

    /// Mutable flat slice of all pixel components.
    pub fn pixels_mut(&mut self) -> &mut [f32] { &mut self.data }

    /// Total number of f32 components (width * height * 3).
    pub fn total_components(&self) -> usize { self.data.len() }

    /// Read-only view of scan-line `y` (0-indexed).
    pub fn row(&self, y: u32) -> &[f32] {
        let start = (y as usize) * (self.width as usize) * 3;
        let end = start + (self.width as usize) * 3;
        &self.data[start..end]
    }

    /// Mutable view of scan-line `y` (0-indexed).
    pub fn row_mut(&mut self, y: u32) -> &mut [f32] {
        let stride = (self.width as usize) * 3;
        let start = (y as usize) * stride;
        let end = start + stride;
        &mut self.data[start..end]
    }

    pub fn size(&self) -> ImageSize {
        ImageSize { width: self.width, height: self.height }
    }

    /// Consume the image and return the underlying flat buffer.
    pub fn into_data(self) -> Vec<f32> { self.data }
}

/// Image dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageSize {
    pub width: u32,
    pub height: u32,
}
```

- [ ] **Step 2: Create `types/status.rs`**

Create file `crates/r3sizer-core/src/types/status.rs` with this exact content:

```rust
use serde::{Deserialize, Serialize};

/// Status of the polynomial fit attempt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum FitStatus {
    /// Cubic polynomial was fitted successfully.
    Success,
    /// Fitting failed for a numerical or data reason.
    Failed { reason: String },
    /// Fitting was skipped (e.g. DirectSearch strategy).
    Skipped,
}

/// Whether the polynomial crossing P_hat(s*) = P0 was found in the probe interval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrossingStatus {
    /// A root was found inside [s_min, s_max].
    Found,
    /// No crossing exists inside the probed interval.
    NotFoundInRange,
    /// Polynomial fit was not attempted or failed; crossing search was skipped.
    NotAttempted,
}

/// How the final sharpening strength was selected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionMode {
    /// Optimal s* from the cubic polynomial root.
    PolynomialRoot,
    /// Polynomial root not available; selected largest probe sample within artifact budget.
    BestSampleWithinBudget,
    /// All probe samples exceed budget; selected the sample with the smallest metric value.
    LeastBadSample,
    /// Budget is structurally unreachable (e.g. baseline already exceeds target in absolute mode).
    BudgetUnreachable,
}
```

- [ ] **Step 3: Create `types/params.rs`**

Create file `crates/r3sizer-core/src/types/params.rs` with this exact content:

```rust
use serde::{Deserialize, Serialize};

use crate::CoreError;

/// How sharpening is applied to the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharpenMode {
    /// Apply unsharp mask directly to all RGB channels.
    Rgb,
    /// Apply unsharp mask to CIE Y lightness, reconstruct RGB via multiplicative
    /// ratio `k = L'/L`.
    ///
    /// Engineering approximation -- the reconstruction formula is a strong inference
    /// from the paper, not a confirmed exact formula.
    Lightness,
}

/// How the artifact metric is computed for sharpness selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricMode {
    /// P_total(s): absolute fraction of channel values outside [0,1].
    /// Includes artifacts from both the resize stage and the sharpen stage.
    AbsoluteTotal,
    /// max(0, P_total(s) - P_base): additional artifacts attributable to sharpening.
    ///
    /// Engineering approximation -- assumes resize and sharpen artifacts are approximately
    /// additive and independent, which is not guaranteed.
    RelativeToBase,
}

/// Controls which sharpening strengths are probed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProbeConfig {
    /// `count` values linearly spaced over `[min, max]`.
    Range { min: f32, max: f32, count: usize },
    /// Caller-supplied explicit list (must have >= 4 distinct, positive values).
    Explicit(Vec<f32>),
}

impl ProbeConfig {
    /// Resolve to a sorted `Vec<f32>`.
    pub fn resolve(&self) -> Result<Vec<f32>, CoreError> {
        let mut values = match self {
            ProbeConfig::Range { min, max, count } => {
                if *count < 4 {
                    return Err(CoreError::InvalidParams(
                        "probe range must have at least 4 samples".into(),
                    ));
                }
                if min >= max {
                    return Err(CoreError::InvalidParams(
                        "probe range min must be less than max".into(),
                    ));
                }
                if *min <= 0.0 {
                    return Err(CoreError::InvalidParams(
                        "probe range min must be positive".into(),
                    ));
                }
                let n = *count;
                (0..n)
                    .map(|i| min + (max - min) * (i as f32) / ((n - 1) as f32))
                    .collect::<Vec<f32>>()
            }
            ProbeConfig::Explicit(v) => {
                if v.len() < 4 {
                    return Err(CoreError::InvalidParams(
                        "explicit probe list must have at least 4 values".into(),
                    ));
                }
                v.clone()
            }
        };
        values.sort_by(|a, b| a.partial_cmp(b).unwrap());
        Ok(values)
    }
}

/// Polynomial fit strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FitStrategy {
    /// Least-squares cubic fit; fall back to direct sampled search if numerically unstable.
    Cubic,
    /// Skip fitting; pick best strength directly from probe samples.
    DirectSearch,
}

/// How to handle out-of-range values at the final output stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClampPolicy {
    /// Hard clamp: values < 0.0 -> 0.0, values > 1.0 -> 1.0.
    Clamp,
    /// Rescale entire image by its global maximum.
    Normalize,
}

/// All parameters controlling the auto-sharpness downscale pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoSharpParams {
    pub target_width: u32,
    pub target_height: u32,
    /// How to select sharpening probe strengths.
    pub probe_strengths: ProbeConfig,
    /// Target artifact ratio P0 (fraction of channel values outside [0,1]).
    /// Default: 0.001 (= 0.1%).
    pub target_artifact_ratio: f32,
    /// Enable the contrast-leveling post-process stage.
    pub enable_contrast_leveling: bool,
    /// Unsharp-mask Gaussian sigma.
    ///
    /// Controls the spatial scale of the sharpening kernel. A value of `1.0`
    /// (default) is a reasonable starting point; increase for larger images or
    /// broader edge enhancement.
    pub sharpen_sigma: f32,
    pub fit_strategy: FitStrategy,
    pub output_clamp: ClampPolicy,
    /// Whether to sharpen RGB directly or through lightness channel.
    pub sharpen_mode: SharpenMode,
    /// How the artifact metric is computed for strength selection.
    pub metric_mode: MetricMode,
}

impl Default for AutoSharpParams {
    fn default() -> Self {
        Self {
            target_width: 800,
            target_height: 600,
            probe_strengths: ProbeConfig::Explicit(
                vec![0.05, 0.1, 0.2, 0.4, 0.8, 1.5, 3.0],
            ),
            target_artifact_ratio: 0.001,
            enable_contrast_leveling: false,
            sharpen_sigma: 1.0,
            fit_strategy: FitStrategy::Cubic,
            output_clamp: ClampPolicy::Clamp,
            sharpen_mode: SharpenMode::Lightness,
            metric_mode: MetricMode::RelativeToBase,
        }
    }
}

impl AutoSharpParams {
    /// Validate that parameters are internally consistent. Called at pipeline entry.
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.target_width == 0 || self.target_height == 0 {
            return Err(CoreError::InvalidParams("target dimensions must be non-zero".into()));
        }
        if self.target_artifact_ratio < 0.0 || self.target_artifact_ratio > 1.0 {
            return Err(CoreError::InvalidParams(
                "target_artifact_ratio must be in [0, 1]".into(),
            ));
        }
        if self.sharpen_sigma <= 0.0 {
            return Err(CoreError::InvalidParams("sharpen_sigma must be positive".into()));
        }
        self.probe_strengths.resolve()?;
        Ok(())
    }
}
```

- [ ] **Step 4: Create `types/results.rs`**

Create file `crates/r3sizer-core/src/types/results.rs` with this exact content:

```rust
use serde::{Deserialize, Serialize};

use super::{
    image::{ImageSize, LinearRgbImage},
    params::{MetricMode, SharpenMode},
    status::{CrossingStatus, FitStatus, SelectionMode},
};

/// A single measured sample of the artifact-vs-strength relationship.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ProbeSample {
    /// Sharpening strength `s`.
    pub strength: f32,
    /// P_total(s): fraction of channel components outside [0, 1] after sharpening.
    pub artifact_ratio: f32,
    /// The metric value used for fitting and selection, depending on `MetricMode`:
    /// - `AbsoluteTotal`: same as `artifact_ratio`
    /// - `RelativeToBase`: `max(0, artifact_ratio - baseline)`
    pub metric_value: f32,
}

/// Cubic polynomial in f64 arithmetic (for numerical stability).
///
/// `P_hat(s) = a*s^3 + b*s^2 + c*s + d`
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CubicPolynomial {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
}

impl CubicPolynomial {
    pub fn evaluate(&self, s: f64) -> f64 {
        self.a * s.powi(3) + self.b * s.powi(2) + self.c * s + self.d
    }
}

/// Diagnostics emitted by the pipeline; serializable for CLI JSON output and GUI display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoSharpDiagnostics {
    // --- Size ---
    pub input_size: ImageSize,
    pub output_size: ImageSize,

    // --- Configuration ---
    pub sharpen_mode: SharpenMode,
    pub metric_mode: MetricMode,
    pub target_artifact_ratio: f32,

    // --- Baseline (resize-stage artifact contribution) ---
    /// Artifact ratio of the downscaled image before any sharpening is applied.
    pub baseline_artifact_ratio: f32,

    // --- Probe data ---
    pub probe_samples: Vec<ProbeSample>,

    // --- Fit / solve results ---
    pub fit_status: FitStatus,
    pub fit_coefficients: Option<CubicPolynomial>,
    pub crossing_status: CrossingStatus,

    // --- Selection result ---
    /// Sharpening strength that was applied to produce the final image.
    pub selected_strength: f32,
    pub selection_mode: SelectionMode,
    /// Whether the target artifact budget is achievable given the baseline and probe range.
    pub budget_reachable: bool,

    // --- Final measurement (pre-clamp) ---
    /// P_total(s*) on the final sharpened image, before clamping.
    pub measured_artifact_ratio: f32,
    /// Metric value of the final output (relative or absolute depending on mode).
    pub measured_metric_value: f32,
}

/// Return type of the top-level pipeline function.
pub struct ProcessOutput {
    /// Final processed image (clamped according to `ClampPolicy`).
    pub image: LinearRgbImage,
    pub diagnostics: AutoSharpDiagnostics,
}
```

- [ ] **Step 5: Create `types/mod.rs`**

Create file `crates/r3sizer-core/src/types/mod.rs` with this exact content:

```rust
//! Shared data types for the r3sizer-core pipeline.
//!
//! Organised into four focused sub-modules:
//!
//! - [`image`] — `LinearRgbImage`, `ImageSize`
//! - [`params`] — pipeline configuration (`AutoSharpParams`, `ProbeConfig`, enums)
//! - [`status`] — solver/diagnostics status enums (`FitStatus`, `CrossingStatus`, `SelectionMode`)
//! - [`results`] — probe and pipeline output types (`ProbeSample`, `CubicPolynomial`, diagnostics)

pub mod image;
pub mod params;
pub mod results;
pub mod status;

// Re-export the complete public surface so that all existing `use crate::types::Foo`
// and `use crate::Foo` (via lib.rs re-exports) continue to work without changes.
pub use image::{ImageSize, LinearRgbImage};
pub use params::{
    AutoSharpParams, ClampPolicy, FitStrategy, MetricMode, ProbeConfig, SharpenMode,
};
pub use results::{AutoSharpDiagnostics, CubicPolynomial, ProcessOutput, ProbeSample};
pub use status::{CrossingStatus, FitStatus, SelectionMode};
```

- [ ] **Step 6: Delete the old `types.rs`**

```bash
rm crates/r3sizer-core/src/types.rs
```

- [ ] **Step 7: Build and test**

```bash
cargo test --workspace && cargo clippy --workspace -- -D warnings
```
Expected: all pass. The split is purely internal — all public re-exports are identical.

- [ ] **Step 8: Commit**

```bash
git add crates/r3sizer-core/src/types/ crates/r3sizer-core/src/
git commit -m "refactor: split types.rs into image/params/status/results submodules"
```

---

## Self-Review

### Spec coverage

| Issue from review | Task |
|---|---|
| `serde_json` in `r3sizer-io` | Task 1 |
| `CoreError::NoValidRoot` underdocumented | Task 2 |
| `FitStrategy::ForcedLinear` dead variant | Task 3 |
| `to_linear_inplace`/`to_srgb_inplace` in wrong module | Task 4 |
| Probe loop allocates N times | Task 5 |
| `types.rs` god-file | Task 6 |

Minor observations from review not planned here (acceptable to defer):
- `proptest` for `fit.rs`/`solve.rs` — requires adding dev-dependency; scope as a separate task
- `sharpen_sigma` CLI help text — acceptable CLI polish, not an architectural issue

### Placeholder scan

No TBD, TODO, or "implement later" patterns found in plan.

### Type consistency

- `unsharp_mask_inplace` / `unsharp_mask_single_channel_inplace` added in Task 5 Step 1
- `reconstruct_rgb_from_lightness_inplace` added in Task 5 Step 2
- `sharpen_image_inplace` in Task 5 Step 3 calls both — names match exactly
- Task 5 Step 4 probe-loop calls `sharpen_image_inplace` — matches the signature defined in Step 3
- Task 6 `types/results.rs` imports `super::image`, `super::params`, `super::status` — these are the exact module names declared in `types/mod.rs`
