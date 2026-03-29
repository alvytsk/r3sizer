# v0.4 Experimental — Test Strategy

## Goal

Verify that each experimental branch (A, B, C, D) works end-to-end through the
WASM pipeline and web UI. Collect concrete numbers to decide which branches
are worth developing further.

## Prerequisites

- Docker image built with `experimental` feature (`docker build -f web/Dockerfile -t r3sizer-web .`)
- Container running: `docker run -p 8080:80 r3sizer-web`
- **Two test images** prepared (any real photo, >= 512x512):
  - `photo-color.jpg` — colorful photo with saturated areas (flowers, sunset, etc.)
  - `photo-detail.jpg` — photo with fine detail and edges (architecture, text, fabric)
- Target size: 256x256 (aggressive downscale to make artifacts visible)
- All tests use the web UI at `http://localhost:8080/r3sizer`

## Recording template

For each test, record from the diagnostics panel:

```
Test:                  <test name>
selected_strength:     <s*>
measured_artifact_ratio: <P>
selection_mode:        <polynomial_root | best_sample_within_budget | ...>
baseline_artifact_ratio: <P_base>
timing.total_us:       <us>
notes:                 <anything unexpected>
```

Save the full diagnostics JSON (JSON tab → copy) to `docs/results/v04/<test_name>.json`.

---

## Test 1: Baseline (control)

**Purpose:** Establish reference values with zero experimental features.

| Step | Setting |
|------|---------|
| 1 | Upload `photo-color.jpg`, set 256x256 |
| 2 | All Experimental section options: Default / Off |
| 3 | Process |

**Record:** `selected_strength`, `measured_artifact_ratio`, `baseline_artifact_ratio`,
full timing breakdown. Save JSON as `baseline-color.json`.

**Repeat** with `photo-detail.jpg` → `baseline-detail.json`.

These two JSONs are the reference for every comparison below.

---

## Test 2: Evaluation Color Space (Branch D — critical wiring test)

**Purpose:** Confirm that changing the evaluation color space actually changes the
fitted curve and selected strength. This was the critical bug that was fixed.

### 2a. Luma Only

| Step | Setting |
|------|---------|
| 1 | Upload `photo-color.jpg`, set 256x256 |
| 2 | Experimental → Evaluation Color Space → **Luma Only** |
| 3 | Process |

**Expected:**
- `measured_artifact_ratio` DIFFERS from baseline (different metric space)
- `selected_strength` DIFFERS from baseline
- All other diagnostics are finite and non-NaN

**Record:** Save JSON as `eval-luma-color.json`.

### 2b. Lab Approx

Same as 2a but set Evaluation Color Space → **Lab Approx**.

**Expected:**
- `measured_artifact_ratio` DIFFERS from both baseline and Luma Only
- `selected_strength` DIFFERS from baseline

**Record:** Save JSON as `eval-lab-color.json`.

### 2c. RGB (explicit)

Set Evaluation Color Space → **RGB**.

**Expected:**
- `selected_strength` MATCHES baseline within 1e-6 (RGB is the default metric)

**Record:** Save JSON as `eval-rgb-color.json`.

### Pass criteria

| Comparison | `selected_strength` | `measured_artifact_ratio` |
|---|---|---|
| baseline vs luma_only | different | different |
| baseline vs lab_approx | different | different |
| baseline vs rgb (explicit) | same (< 1e-6) | same (< 1e-6) |

**If baseline == luma_only**: the wiring is broken — `evaluate_in_color_space` is not
being called from the pipeline. Check that the WASM was built with `experimental`.

---

## Test 3: Chroma Guard Sharpening (Branch D)

**Purpose:** Verify chroma guard produces a different output image and populates diagnostics.

### 3a. Default chroma shift

| Step | Setting |
|------|---------|
| 1 | Upload `photo-color.jpg`, set 256x256 |
| 2 | Experimental → Sharpen Mode → **Luma + Chroma Guard** (default 0.10) |
| 3 | Process |

**Expected:**
- Diagnostics panel: **Chroma Guard** card appears
- `pixels_clamped_fraction` > 0 (some pixels had chroma shift above threshold)
- `mean_chroma_shift` > 0
- Output image visually differs from baseline (check comparison slider)
- `selected_strength` may or may not differ (chroma guard only affects the final
  sharpen, not the probing)

**Record:** `pixels_clamped_fraction`, `mean_chroma_shift`, `max_chroma_shift`.
Save JSON as `chroma-guard-color.json`.

### 3b. Tight chroma shift (0.02)

Repeat 3a with Max Chroma Shift slider → **0.02**.

**Expected:**
- `pixels_clamped_fraction` HIGHER than 3a (more pixels clamped)
- Output image should look more muted in chroma than 3a

**Record:** Save JSON as `chroma-guard-tight-color.json`.

### 3c. Loose chroma shift (0.50)

Repeat 3a with Max Chroma Shift slider → **0.50**.

**Expected:**
- `pixels_clamped_fraction` LOWER than 3a (fewer pixels clamped)
- Could be zero if no pixel has > 50% chroma shift

**Record:** Save JSON as `chroma-guard-loose-color.json`.

### Pass criteria

| Setting | `pixels_clamped_fraction` |
|---|---|
| 0.02 (tight) | highest |
| 0.10 (default) | medium |
| 0.50 (loose) | lowest |

The three values must be monotonically non-increasing as the threshold loosens.

---

## Test 4: Resize Strategy (Branch B)

**Purpose:** Verify alternative resize kernels produce different outputs.

### 4a. Uniform CatmullRom

| Step | Setting |
|------|---------|
| 1 | Upload `photo-detail.jpg`, set 256x256 |
| 2 | Experimental → Resize Strategy → **Uniform**, Kernel → **Catmull-Rom** |
| 3 | Process |

**Expected:**
- Diagnostics panel: **Resize Strategy** card appears
- `kernels_used` = `["catmull_rom"]`
- `selected_strength` likely DIFFERS from baseline (different downscale = different
  baseline artifact level)
- `baseline_artifact_ratio` DIFFERS from baseline (different kernel → different
  pre-sharpen artifacts)

**Record:** `baseline_artifact_ratio`, `selected_strength`. Save JSON as `resize-catmull-detail.json`.

### 4b. Uniform Gaussian

Same as 4a but Kernel → **Gaussian**.

**Expected:**
- `baseline_artifact_ratio` likely lower than Lanczos3 (Gaussian is smoother)
- `selected_strength` likely higher (smoother base → can sharpen more)

**Record:** Save JSON as `resize-gaussian-detail.json`.

### 4c. Uniform MitchellNetravali

Same as 4a but Kernel → **Mitchell-Netravali**.

**Record:** Save JSON as `resize-mitchell-detail.json`.

### 4d. Content Adaptive (default kernel table)

| Step | Setting |
|------|---------|
| 1 | Upload `photo-detail.jpg`, set 256x256 |
| 2 | Experimental → Resize Strategy → **Content Adaptive** (default kernel table) |
| 3 | Process |

**Expected:**
- `kernels_used` has multiple entries (e.g. `["gaussian", "lanczos3", "catmull_rom"]`)
- `per_kernel_pixel_count` shows per-kernel breakdown

**Record:** `kernels_used`, `per_kernel_pixel_count`. Save JSON as `resize-adaptive-detail.json`.

### Pass criteria

| Kernel | `baseline_artifact_ratio` |
|---|---|
| Lanczos3 (baseline) | reference |
| CatmullRom | different from reference |
| Gaussian | different from reference |
| MitchellNetravali | different from reference |

All four must be distinct. If any matches baseline exactly, the kernel dispatch is broken.

---

## Test 5: Input Color Space (Branch C)

**Purpose:** Verify ingress diagnostics appear and LinearRgb/sRGB paths differ.

### 5a. sRGB (explicit)

| Step | Setting |
|------|---------|
| 1 | Upload `photo-color.jpg`, set 256x256 |
| 2 | Experimental → Input Color Space → **sRGB** |
| 3 | Process |

**Expected:**
- Diagnostics panel: **Ingress** card appears
- `declared_color_space` = `srgb`
- `selected_strength` matches baseline (sRGB is the default)
- `normalization_scale` = null, `out_of_range_fraction` = null

**Record:** Save JSON as `ingress-srgb-color.json`.

### 5b. Linear RGB

Set Input Color Space → **Linear RGB**.

**Expected:**
- `declared_color_space` = `linear_rgb`
- `selected_strength` DIFFERS from baseline (the pipeline skips sRGB→linear
  conversion, so pixel values are reinterpreted)
- `out_of_range_fraction` may be populated if values > 1.0

**Record:** Save JSON as `ingress-linear-color.json`.

### 5c. RAW Linear

Set Input Color Space → **RAW Linear**.

**Expected:**
- `declared_color_space` = `raw_linear`
- `normalization_scale` is populated (even though the input is 0-1 sRGB data,
  the pipeline treats it as potentially HDR and normalizes)
- Ingress timing is populated

**Record:** Save JSON as `ingress-raw-color.json`.

### Pass criteria

- sRGB `selected_strength` matches baseline
- Linear RGB `selected_strength` differs (proves ingress actually modifies behavior)
- RAW Linear shows `normalization_scale` in diagnostics

---

## Test 6: Quality Evaluator (Branch A)

**Purpose:** Verify heuristic evaluator produces diagnostics without affecting output.

### 6a. Evaluator on

| Step | Setting |
|------|---------|
| 1 | Upload `photo-detail.jpg`, set 256x256 |
| 2 | Experimental → Quality Evaluator → **Heuristic** |
| 3 | Process |

**Expected:**
- Diagnostics panel: **Quality Evaluator** card appears
- `predicted_quality_score` in [0, 1]
- `confidence` in [0, 1]
- `features` section expandable with 7 finite values
- `selected_strength` MATCHES baseline (evaluator is advisory only)
- Timing bar shows **Evaluator** row

**Record:** `predicted_quality_score`, `confidence`, `suggested_strength`,
all 7 features. Save JSON as `evaluator-detail.json`.

### 6b. Evaluator off (control)

Remove evaluator (set to Off).

**Expected:**
- `evaluator_result` absent from diagnostics
- `selected_strength` matches 6a exactly

### 6c. Features differ between images

Run evaluator on `photo-color.jpg`.

**Expected:**
- `features` values differ from `photo-detail.jpg` (different image content →
  different edge density, gradient stats, etc.)

**Record:** Save JSON as `evaluator-color.json`.

### Pass criteria

- `selected_strength` with evaluator ON matches OFF (advisory only)
- `predicted_quality_score` is finite and in [0, 1]
- Features differ between two test images (at least 4 of 7 features differ by > 10%)

---

## Test 7: All features together

**Purpose:** Verify no interaction bugs when all experimental features are active.

| Step | Setting |
|------|---------|
| 1 | Upload `photo-color.jpg`, set 256x256 |
| 2 | Input Color Space → **Linear RGB** |
| 3 | Resize Strategy → **Uniform**, Kernel → **Catmull-Rom** |
| 4 | Sharpen Mode → **Luma + Chroma Guard** (0.10) |
| 5 | Evaluation Color Space → **Luma Only** |
| 6 | Quality Evaluator → **Heuristic** |
| 7 | Process |

**Expected:**
- No error, output image renders
- ALL 4 experimental diagnostic cards appear:
  - Ingress (declared_color_space = linear_rgb)
  - Resize Strategy (kernels_used = ["catmull_rom"])
  - Chroma Guard (pixels_clamped_fraction > 0)
  - Quality Evaluator (predicted_quality_score in [0,1])
- Timing bar shows both **Ingress** and **Evaluator** rows
- `ingress_us` and `evaluator_us` are populated in timing

**Record:** Save JSON as `all-features-color.json`.

### Pass criteria

- Pipeline completes without error
- 4 diagnostic cards visible
- All numeric values are finite

---

## Summary matrix

After running all tests, fill in this table:

| Test | Image | `selected_strength` | `measured_artifact_ratio` | `baseline_artifact_ratio` | Key diagnostic value | Pass? |
|------|-------|--------------------:|-------------------------:|-------------------------:|---------------------|-------|
| 1. Baseline | color | | | | — | ref |
| 1. Baseline | detail | | | | — | ref |
| 2a. Eval Luma | color | | | | differs from T1 | |
| 2b. Eval Lab | color | | | | differs from T1 | |
| 2c. Eval RGB | color | | | | matches T1 | |
| 3a. Chroma 0.10 | color | | | | clamped_frac | |
| 3b. Chroma 0.02 | color | | | | clamped_frac > 3a | |
| 3c. Chroma 0.50 | color | | | | clamped_frac < 3a | |
| 4a. CatmullRom | detail | | | | differs from T1 | |
| 4b. Gaussian | detail | | | | differs from T1 | |
| 4c. Mitchell | detail | | | | differs from T1 | |
| 4d. Adaptive | detail | | | | kernels_used.len > 1 | |
| 5a. sRGB | color | | | | matches T1 | |
| 5b. Linear RGB | color | | | | differs from T1 | |
| 5c. RAW Linear | color | | | | norm_scale set | |
| 6a. Evaluator ON | detail | | | | score in [0,1] | |
| 6c. Evaluator img2 | color | | | | features differ | |
| 7. All together | color | | | | 4 cards visible | |

## Decision criteria

After filling the table:

- **Branch worth keeping**: all its tests pass, produces meaningfully different results
- **Branch needs fixing**: tests fail or numbers don't change when they should
- **Branch to drop**: works correctly but produces negligible differences on real photos

The most critical result is **Test 2a vs Test 1**: if `selected_strength` is
identical, the `EvaluationColorSpace` pipeline wiring is not working end-to-end
and must be debugged before anything else.
