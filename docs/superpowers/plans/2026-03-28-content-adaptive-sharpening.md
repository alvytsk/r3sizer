# Content-Adaptive Sharpening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Introduce per-pixel sharpening gain modulated by region classification (flat, textured, strong edge, microtexture, risky halo zone) with a "global solve, local apply, validated final" pipeline contract.

**Architecture:** A new `classifier.rs` module classifies each pixel of the pre-sharpen working image into one of five region classes using Sobel gradient magnitude and local variance. This produces a `GainMap` that modulates the global sharpening strength `s*` at the final-apply step. A validation/backoff loop ensures the adaptive result stays within the artifact budget. The probe/fit/solve loop is unchanged (uniform probing).

**Tech Stack:** Rust (r3sizer-core crate), no new dependencies. Uses existing `LinearRgbImage`, serde derives, `web_time::Instant` for timing.

**Spec:** `docs/superpowers/specs/2026-03-28-content-adaptive-sharpening-design.md`

---

## File Map

| Action | File | Responsibility |
|--------|------|----------------|
| Modify | `crates/r3sizer-core/src/types.rs` | Add `RegionClass`, `RegionMap`, `GainMap`, `GainTable`, `ClassificationParams`, `SharpenStrategy`, `RegionCoverage`, `AdaptiveValidationOutcome`; extend `AutoSharpParams`, `AutoSharpDiagnostics`, `StageTiming` |
| Modify | `crates/r3sizer-core/src/lib.rs` | Register `classifier` module; add new types to re-exports |
| Create | `crates/r3sizer-core/src/classifier.rs` | `classify()`, `classify_features()`, `gain_map_from_region_map()`, Sobel gradient, local variance |
| Modify | `crates/r3sizer-core/src/sharpen.rs` | Add `adaptive_sharpen_lightness()`, `adaptive_sharpen_rgb()` |
| Modify | `crates/r3sizer-core/src/pipeline.rs` | Stage 2.5 classification, Stage 9 adaptive branch, Stage 9.5 validation/backoff, diagnostics assembly |
| Modify | `crates/r3sizer-core/tests/integration.rs` | Uniform regression guard, ContentAdaptive happy path, backoff tests, determinism |

---

### Task 1: New types — `RegionClass`, `RegionMap`, `GainMap`

**Files:**
- Modify: `crates/r3sizer-core/src/types.rs`

- [ ] **Step 1: Write tests for RegionClass, RegionMap, GainMap**

Add at the bottom of `types.rs`, inside a new `#[cfg(test)] mod adaptive_tests`:

```rust
#[cfg(test)]
mod adaptive_tests {
    use super::*;

    #[test]
    fn region_class_as_usize_stable_ordering() {
        assert_eq!(RegionClass::Flat as usize, 0);
        assert_eq!(RegionClass::Textured as usize, 1);
        assert_eq!(RegionClass::StrongEdge as usize, 2);
        assert_eq!(RegionClass::Microtexture as usize, 3);
        assert_eq!(RegionClass::RiskyHaloZone as usize, 4);
    }

    #[test]
    fn region_map_valid_construction() {
        let data = vec![RegionClass::Flat; 12];
        let map = RegionMap::new(4, 3, data).unwrap();
        assert_eq!(map.width, 4);
        assert_eq!(map.height, 3);
        assert_eq!(map.get(0, 0), RegionClass::Flat);
        assert_eq!(map.get(3, 2), RegionClass::Flat);
    }

    #[test]
    fn region_map_wrong_length_fails() {
        let data = vec![RegionClass::Flat; 10];
        assert!(RegionMap::new(4, 3, data).is_err());
    }

    #[test]
    fn gain_map_valid_construction() {
        let data = vec![1.0f32; 6];
        let map = GainMap::new(3, 2, data).unwrap();
        assert_eq!(map.width, 3);
        assert_eq!(map.height, 2);
        assert!((map.get(0, 0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn gain_map_wrong_length_fails() {
        let data = vec![1.0f32; 5];
        assert!(GainMap::new(3, 2, data).is_err());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p r3sizer-core --lib adaptive_tests -- --nocapture`
Expected: FAIL — `RegionClass`, `RegionMap`, `GainMap` not defined.

- [ ] **Step 3: Implement RegionClass, RegionMap, GainMap**

Add to `types.rs` after the `MetricComponent` section (around line 463), before `MetricBreakdown`:

```rust
// ---------------------------------------------------------------------------
// Content-adaptive sharpening types (v0.3)
// ---------------------------------------------------------------------------

/// Number of region classes.
pub const REGION_CLASS_COUNT: usize = 5;

/// Classification of a pixel's local content for adaptive sharpening.
///
/// Stable `as usize` ordering is part of the public contract:
/// Flat=0, Textured=1, StrongEdge=2, Microtexture=3, RiskyHaloZone=4.
///
/// Provenance: `EngineeringChoice` — taxonomy is not paper-confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum RegionClass {
    Flat = 0,
    Textured = 1,
    StrongEdge = 2,
    Microtexture = 3,
    RiskyHaloZone = 4,
}

/// Per-pixel region classification map with embedded dimensions.
///
/// Dimensions are part of the type to prevent accidental reuse with
/// a wrong-sized image.
#[derive(Debug, Clone)]
pub struct RegionMap {
    pub width: u32,
    pub height: u32,
    data: Vec<RegionClass>,
}

impl RegionMap {
    /// Create a new region map. Returns error if `data.len() != width * height`.
    pub fn new(width: u32, height: u32, data: Vec<RegionClass>) -> Result<Self, CoreError> {
        let expected = (width as usize) * (height as usize);
        if data.len() != expected {
            return Err(CoreError::BufferLengthMismatch {
                expected_len: expected,
                got_len: data.len(),
            });
        }
        Ok(Self { width, height, data })
    }

    /// Read the class at pixel (x, y).
    #[inline]
    pub fn get(&self, x: u32, y: u32) -> RegionClass {
        self.data[(y as usize) * (self.width as usize) + (x as usize)]
    }

    /// Read-only access to the underlying data slice.
    pub fn data(&self) -> &[RegionClass] {
        &self.data
    }
}

/// Per-pixel gain multiplier map with embedded dimensions.
#[derive(Debug, Clone)]
pub struct GainMap {
    pub width: u32,
    pub height: u32,
    data: Vec<f32>,
}

impl GainMap {
    /// Create a new gain map. Returns error if `data.len() != width * height`.
    pub fn new(width: u32, height: u32, data: Vec<f32>) -> Result<Self, CoreError> {
        let expected = (width as usize) * (height as usize);
        if data.len() != expected {
            return Err(CoreError::BufferLengthMismatch {
                expected_len: expected,
                got_len: data.len(),
            });
        }
        Ok(Self { width, height, data })
    }

    /// Read the gain at pixel (x, y).
    #[inline]
    pub fn get(&self, x: u32, y: u32) -> f32 {
        self.data[(y as usize) * (self.width as usize) + (x as usize)]
    }

    /// Read-only access to the underlying data slice.
    pub fn data(&self) -> &[f32] {
        &self.data
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p r3sizer-core --lib adaptive_tests -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/r3sizer-core/src/types.rs
git commit -m "feat(types): add RegionClass, RegionMap, GainMap for v0.3 adaptive sharpening"
```

---

### Task 2: New types — `GainTable`, `ClassificationParams`

**Files:**
- Modify: `crates/r3sizer-core/src/types.rs`

- [ ] **Step 1: Write tests for GainTable and ClassificationParams**

Add to the existing `adaptive_tests` module in `types.rs`:

```rust
    #[test]
    fn gain_table_v03_default_values() {
        let gt = GainTable::v03_default();
        assert!((gt.flat - 0.75).abs() < 1e-6);
        assert!((gt.textured - 0.95).abs() < 1e-6);
        assert!((gt.strong_edge - 1.00).abs() < 1e-6);
        assert!((gt.microtexture - 1.10).abs() < 1e-6);
        assert!((gt.risky_halo_zone - 0.70).abs() < 1e-6);
    }

    #[test]
    fn gain_table_gain_for_each_class() {
        let gt = GainTable::v03_default();
        assert!((gt.gain_for(RegionClass::Flat) - 0.75).abs() < 1e-6);
        assert!((gt.gain_for(RegionClass::Textured) - 0.95).abs() < 1e-6);
        assert!((gt.gain_for(RegionClass::StrongEdge) - 1.00).abs() < 1e-6);
        assert!((gt.gain_for(RegionClass::Microtexture) - 1.10).abs() < 1e-6);
        assert!((gt.gain_for(RegionClass::RiskyHaloZone) - 0.70).abs() < 1e-6);
    }

    #[test]
    fn gain_table_out_of_bounds_rejected() {
        assert!(GainTable::new(0.2, 1.0, 1.0, 1.0, 1.0).is_err()); // below 0.25
        assert!(GainTable::new(1.0, 5.0, 1.0, 1.0, 1.0).is_err()); // above 4.0
    }

    #[test]
    fn gain_table_at_bounds_accepted() {
        assert!(GainTable::new(0.25, 4.0, 1.0, 1.0, 1.0).is_ok());
    }

    #[test]
    fn classification_params_default_valid() {
        let cp = ClassificationParams::default();
        assert!(cp.gradient_low_threshold <= cp.gradient_high_threshold);
        assert!(cp.variance_low_threshold <= cp.variance_high_threshold);
        assert!(cp.variance_window >= 3);
        assert!(cp.variance_window % 2 == 1);
    }

    #[test]
    fn classification_params_inverted_gradient_rejected() {
        let result = ClassificationParams::new(0.5, 0.1, 0.001, 0.01, 5);
        assert!(result.is_err());
    }

    #[test]
    fn classification_params_inverted_variance_rejected() {
        let result = ClassificationParams::new(0.05, 0.4, 0.1, 0.01, 5);
        assert!(result.is_err());
    }

    #[test]
    fn classification_params_even_window_rejected() {
        let result = ClassificationParams::new(0.05, 0.4, 0.001, 0.01, 4);
        assert!(result.is_err());
    }

    #[test]
    fn classification_params_window_too_small_rejected() {
        let result = ClassificationParams::new(0.05, 0.4, 0.001, 0.01, 1);
        assert!(result.is_err());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p r3sizer-core --lib adaptive_tests -- --nocapture`
Expected: FAIL — `GainTable`, `ClassificationParams` not defined.

- [ ] **Step 3: Implement GainTable and ClassificationParams**

Add to `types.rs` after `GainMap`:

```rust
/// Per-class gain multipliers for adaptive sharpening.
///
/// **Hard validation bound:** all values must be in `[0.25, 4.0]`.
/// This prevents absurd configuration but does not imply values near the
/// bounds are supported or tested.
///
/// **Recommended operating range:** `[0.5, 1.5]`.
///
/// **Design criterion:** misclassification should degrade gently, not dramatically.
///
/// Provenance: `EngineeringChoice`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GainTable {
    pub flat: f32,
    pub textured: f32,
    pub strong_edge: f32,
    pub microtexture: f32,
    pub risky_halo_zone: f32,
}

impl GainTable {
    const MIN_GAIN: f32 = 0.25;
    const MAX_GAIN: f32 = 4.0;

    /// Construct with validation: all values must be in `[0.25, 4.0]`.
    pub fn new(
        flat: f32,
        textured: f32,
        strong_edge: f32,
        microtexture: f32,
        risky_halo_zone: f32,
    ) -> Result<Self, CoreError> {
        let vals = [flat, textured, strong_edge, microtexture, risky_halo_zone];
        for &v in &vals {
            if !(Self::MIN_GAIN..=Self::MAX_GAIN).contains(&v) {
                return Err(CoreError::InvalidParams(format!(
                    "gain value {v} outside allowed range [{}, {}]",
                    Self::MIN_GAIN,
                    Self::MAX_GAIN,
                )));
            }
        }
        Ok(Self { flat, textured, strong_edge, microtexture, risky_halo_zone })
    }

    /// Canonical v0.3 preset. Range `[0.70, 1.10]`.
    pub fn v03_default() -> Self {
        Self {
            flat: 0.75,
            textured: 0.95,
            strong_edge: 1.00,
            microtexture: 1.10,
            risky_halo_zone: 0.70,
        }
    }

    /// Look up the gain for a given region class.
    #[inline]
    pub fn gain_for(&self, class: RegionClass) -> f32 {
        match class {
            RegionClass::Flat => self.flat,
            RegionClass::Textured => self.textured,
            RegionClass::StrongEdge => self.strong_edge,
            RegionClass::Microtexture => self.microtexture,
            RegionClass::RiskyHaloZone => self.risky_halo_zone,
        }
    }
}

/// Thresholds for the four-pass pixel classifier.
///
/// All thresholds are tied to the specific operators in `classifier.rs`:
/// - Gradient thresholds: **unnormalized Sobel scale** (max ≈ 5.66 for luminance in [0,1]).
/// - Variance thresholds: **squared-luminance units** (max 0.25 for bounded data).
///
/// Changing the Sobel normalization or variance formula invalidates these defaults.
///
/// Provenance: `EngineeringChoice`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ClassificationParams {
    pub gradient_low_threshold: f32,
    pub gradient_high_threshold: f32,
    pub variance_low_threshold: f32,
    pub variance_high_threshold: f32,
    pub variance_window: usize,
}

impl ClassificationParams {
    /// Construct with validation.
    pub fn new(
        gradient_low_threshold: f32,
        gradient_high_threshold: f32,
        variance_low_threshold: f32,
        variance_high_threshold: f32,
        variance_window: usize,
    ) -> Result<Self, CoreError> {
        if gradient_low_threshold > gradient_high_threshold {
            return Err(CoreError::InvalidParams(
                "gradient_low_threshold must be <= gradient_high_threshold".into(),
            ));
        }
        if variance_low_threshold > variance_high_threshold {
            return Err(CoreError::InvalidParams(
                "variance_low_threshold must be <= variance_high_threshold".into(),
            ));
        }
        if variance_window < 3 {
            return Err(CoreError::InvalidParams(
                "variance_window must be >= 3".into(),
            ));
        }
        if variance_window % 2 == 0 {
            return Err(CoreError::InvalidParams(
                "variance_window must be odd".into(),
            ));
        }
        Ok(Self {
            gradient_low_threshold,
            gradient_high_threshold,
            variance_low_threshold,
            variance_high_threshold,
            variance_window,
        })
    }
}

impl Default for ClassificationParams {
    fn default() -> Self {
        Self {
            gradient_low_threshold: 0.05,
            gradient_high_threshold: 0.40,
            variance_low_threshold: 0.001,
            variance_high_threshold: 0.010,
            variance_window: 5,
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p r3sizer-core --lib adaptive_tests -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/r3sizer-core/src/types.rs
git commit -m "feat(types): add GainTable, ClassificationParams for v0.3"
```

---

### Task 3: New types — `SharpenStrategy`, `RegionCoverage`, `AdaptiveValidationOutcome`

**Files:**
- Modify: `crates/r3sizer-core/src/types.rs`

- [ ] **Step 1: Write tests**

Add to `adaptive_tests`:

```rust
    #[test]
    fn sharpen_strategy_default_is_uniform() {
        assert!(matches!(SharpenStrategy::default(), SharpenStrategy::Uniform));
    }

    #[test]
    fn sharpen_strategy_content_adaptive_construction() {
        let strategy = SharpenStrategy::ContentAdaptive {
            classification: ClassificationParams::default(),
            gain_table: GainTable::v03_default(),
            max_backoff_iterations: 4,
            backoff_scale_factor: 0.8,
        };
        assert!(matches!(strategy, SharpenStrategy::ContentAdaptive { .. }));
    }

    #[test]
    fn region_coverage_invariant() {
        let rc = RegionCoverage::from_region_map(&RegionMap::new(
            2, 2,
            vec![
                RegionClass::Flat,
                RegionClass::Textured,
                RegionClass::StrongEdge,
                RegionClass::Flat,
            ],
        ).unwrap());
        assert_eq!(rc.total_pixels, 4);
        assert_eq!(rc.flat + rc.textured + rc.strong_edge + rc.microtexture + rc.risky_halo_zone, 4);
        assert_eq!(rc.flat, 2);
        assert_eq!(rc.textured, 1);
        assert_eq!(rc.strong_edge, 1);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p r3sizer-core --lib adaptive_tests -- --nocapture`
Expected: FAIL — types not defined.

- [ ] **Step 3: Implement SharpenStrategy, RegionCoverage, AdaptiveValidationOutcome**

Add to `types.rs` after `ClassificationParams`:

```rust
/// Orchestration axis for sharpening strength distribution.
///
/// Orthogonal to [`SharpenMode`] (Rgb/Lightness) and [`SharpenModel`] (operator).
/// `SharpenStrategy` controls whether strength is applied uniformly or modulated
/// per-pixel by a region-based gain map.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "strategy")]
pub enum SharpenStrategy {
    /// Current behaviour: single global strength applied everywhere.
    Uniform,
    /// Per-pixel gain modulated by region classification.
    ContentAdaptive {
        classification: ClassificationParams,
        gain_table: GainTable,
        /// Maximum backoff iterations if adaptive result exceeds budget. Default: 4.
        max_backoff_iterations: u8,
        /// Scale reduction per backoff iteration. Must be in (0.0, 1.0). Default: 0.8.
        backoff_scale_factor: f32,
    },
}

impl Default for SharpenStrategy {
    fn default() -> Self {
        Self::Uniform
    }
}

/// Per-class pixel coverage computed from a [`RegionMap`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RegionCoverage {
    pub total_pixels: u32,
    pub flat: u32,
    pub textured: u32,
    pub strong_edge: u32,
    pub microtexture: u32,
    pub risky_halo_zone: u32,
    pub flat_fraction: f32,
    pub textured_fraction: f32,
    pub strong_edge_fraction: f32,
    pub microtexture_fraction: f32,
    pub risky_halo_zone_fraction: f32,
}

impl RegionCoverage {
    /// Compute coverage statistics from a region map.
    pub fn from_region_map(map: &RegionMap) -> Self {
        let mut counts = [0u32; REGION_CLASS_COUNT];
        for &c in map.data() {
            counts[c as usize] += 1;
        }
        let total = map.width * map.height;
        let frac = |c: u32| if total > 0 { c as f32 / total as f32 } else { 0.0 };
        Self {
            total_pixels: total,
            flat: counts[0],
            textured: counts[1],
            strong_edge: counts[2],
            microtexture: counts[3],
            risky_halo_zone: counts[4],
            flat_fraction: frac(counts[0]),
            textured_fraction: frac(counts[1]),
            strong_edge_fraction: frac(counts[2]),
            microtexture_fraction: frac(counts[3]),
            risky_halo_zone_fraction: frac(counts[4]),
        }
    }
}

/// Outcome of the adaptive validation / backoff phase.
///
/// `target_metric` is not duplicated here — it lives in [`AutoSharpParams::target_artifact_ratio`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "outcome")]
pub enum AdaptiveValidationOutcome {
    /// Adaptive result met budget on first try.
    PassedDirect { measured_metric: f32 },
    /// Budget met after scaling down global strength.
    PassedAfterBackoff {
        iterations: u8,
        final_scale: f32,
        measured_metric: f32,
    },
    /// Budget not met after all backoff iterations; best result returned.
    FailedBudgetExceeded {
        iterations: u8,
        best_scale: f32,
        best_metric: f32,
    },
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p r3sizer-core --lib adaptive_tests -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/r3sizer-core/src/types.rs
git commit -m "feat(types): add SharpenStrategy, RegionCoverage, AdaptiveValidationOutcome"
```

---

### Task 4: Extend `AutoSharpParams`, `AutoSharpDiagnostics`, `StageTiming`

**Files:**
- Modify: `crates/r3sizer-core/src/types.rs`
- Modify: `crates/r3sizer-core/src/pipeline.rs`

- [ ] **Step 1: Add `sharpen_strategy` to `AutoSharpParams`**

In `types.rs`, add field to `AutoSharpParams` struct (after `diagnostics_level`):

```rust
    /// Strength distribution strategy. Default: `Uniform`.
    pub sharpen_strategy: SharpenStrategy,
```

Add to `Default for AutoSharpParams`:

```rust
            sharpen_strategy: SharpenStrategy::default(),
```

Add validation in `AutoSharpParams::validate()` after the existing checks:

```rust
        if let SharpenStrategy::ContentAdaptive { backoff_scale_factor, .. } = &self.sharpen_strategy {
            if *backoff_scale_factor <= 0.0 || *backoff_scale_factor >= 1.0 {
                return Err(CoreError::InvalidParams(
                    "backoff_scale_factor must be in (0.0, 1.0)".into(),
                ));
            }
        }
```

- [ ] **Step 2: Add adaptive fields to `StageTiming`**

In `types.rs`, add to `StageTiming` struct:

```rust
    /// Region classification time (None when Uniform).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classification_us: Option<u64>,
    /// Adaptive validation + backoff time (None when Uniform).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adaptive_validation_us: Option<u64>,
```

- [ ] **Step 3: Add adaptive fields to `AutoSharpDiagnostics`**

In `types.rs`, add to `AutoSharpDiagnostics` struct (after `metric_weights_provenance`):

```rust
    // --- Content-adaptive (v0.3) ---
    /// Per-class region coverage. None when `SharpenStrategy::Uniform`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region_coverage: Option<RegionCoverage>,
    /// Outcome of adaptive validation. None when `SharpenStrategy::Uniform`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adaptive_validation: Option<AdaptiveValidationOutcome>,
```

- [ ] **Step 4: Update pipeline diagnostics assembly**

In `pipeline.rs`, update the `AutoSharpDiagnostics` construction (around line 321) to include the new fields:

```rust
        region_coverage: None,
        adaptive_validation: None,
```

And update the `StageTiming` construction to include:

```rust
            classification_us: None,
            adaptive_validation_us: None,
```

- [ ] **Step 5: Update imports in pipeline.rs**

Add `SharpenStrategy` to the import block at the top of `pipeline.rs`:

```rust
    ArtifactMetric, AutoSharpDiagnostics, AutoSharpParams, ClampPolicy, FallbackReason,
    FitStatus, FitStrategy, ImageSize, LinearRgbImage, MetricMode, MetricWeights, ProbeSample,
    ProcessOutput, Provenance, RobustnessFlags, SelectionMode, SharpenMode, SharpenModel,
    SharpenStrategy, StageTiming, StageProvenance, CoreError,
```

- [ ] **Step 6: Run full test suite**

Run: `cargo test --workspace`
Expected: PASS — all existing tests compile and pass with new default fields.

Run: `cargo clippy --workspace -- -D warnings`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/r3sizer-core/src/types.rs crates/r3sizer-core/src/pipeline.rs
git commit -m "feat(types): extend AutoSharpParams/Diagnostics/StageTiming for adaptive sharpening"
```

---

### Task 5: Update `lib.rs` re-exports

**Files:**
- Modify: `crates/r3sizer-core/src/lib.rs`

- [ ] **Step 1: Register classifier module and add re-exports**

Add the module declaration to `lib.rs` (after `pub mod contrast;`):

```rust
pub mod classifier;
```

Add to the `pub use types::{ ... }` block:

```rust
    AdaptiveValidationOutcome, ClassificationParams, GainMap, GainTable,
    RegionClass, RegionCoverage, RegionMap, SharpenStrategy,
```

(Don't worry that `classifier` module doesn't exist yet — it will be created in Task 6.)

- [ ] **Step 2: Verify it compiles once classifier.rs exists**

This will be verified after Task 6. For now, note that `cargo check` will fail until the module file is created.

- [ ] **Step 3: Commit**

```bash
git add crates/r3sizer-core/src/lib.rs
git commit -m "feat(lib): register classifier module and re-export v0.3 types"
```

---

### Task 6: `classifier.rs` — classify_features rule + tests

**Files:**
- Create: `crates/r3sizer-core/src/classifier.rs`

- [ ] **Step 1: Create classifier.rs with classify_features and its tests**

Create `crates/r3sizer-core/src/classifier.rs`:

```rust
//! Content-adaptive region classification for v0.3 adaptive sharpening.
//!
//! Self-contained module — does not depend on `metrics/`.
//! Uses the same CIE Y luminance coefficients as `color.rs`
//! (intentionally co-owned; shared constants extracted if duplication spreads).

use crate::{ClassificationParams, CoreError, GainMap, GainTable, LinearRgbImage, RegionClass, RegionMap};

// ---------------------------------------------------------------------------
// Luminance coefficients (co-owned with color.rs)
// ---------------------------------------------------------------------------

/// CIE Y luminance from linear sRGB. Same formula as `color::luminance_from_linear_srgb`.
#[inline]
fn luminance(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

// ---------------------------------------------------------------------------
// Classification rule (exposed for direct testing)
// ---------------------------------------------------------------------------

/// Classify a single pixel from pre-computed features.
///
/// Priority order (part of the public contract):
/// 1. `g >= gradient_high && v >= variance_high` → `RiskyHaloZone`
/// 2. `g >= gradient_high`                       → `StrongEdge`
/// 3. `v >= variance_high && g < gradient_low`   → `Microtexture`
/// 4. `v >= variance_low || g >= gradient_low`    → `Textured`
/// 5. else                                        → `Flat`
pub(crate) fn classify_features(
    gradient_mag: f32,
    variance: f32,
    params: &ClassificationParams,
) -> RegionClass {
    if gradient_mag >= params.gradient_high_threshold && variance >= params.variance_high_threshold {
        RegionClass::RiskyHaloZone
    } else if gradient_mag >= params.gradient_high_threshold {
        RegionClass::StrongEdge
    } else if variance >= params.variance_high_threshold && gradient_mag < params.gradient_low_threshold {
        RegionClass::Microtexture
    } else if variance >= params.variance_low_threshold || gradient_mag >= params.gradient_low_threshold {
        RegionClass::Textured
    } else {
        RegionClass::Flat
    }
}

// Stubs for classify and gain_map_from_region_map — implemented in later tasks.

/// Classify every pixel of a linear RGB image into region classes.
///
/// Four passes over a luminance channel extracted internally:
/// 0. Luminance extraction
/// 1. Sobel gradient magnitude (unnormalized, edge-replicate border)
/// 2. Local variance (square window, edge-replicate border)
/// 3. Per-pixel classification via [`classify_features`]
pub fn classify(
    _image: &LinearRgbImage,
    _params: &ClassificationParams,
) -> RegionMap {
    todo!("implemented in Task 7–9")
}

/// Produce a per-pixel gain map from a region map and gain table.
pub fn gain_map_from_region_map(
    region_map: &RegionMap,
    gain_table: &GainTable,
) -> GainMap {
    let data: Vec<f32> = region_map
        .data()
        .iter()
        .map(|&c| gain_table.gain_for(c))
        .collect();
    GainMap::new(region_map.width, region_map.height, data).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_params() -> ClassificationParams {
        ClassificationParams::default()
    }

    // -----------------------------------------------------------------------
    // Layer (a): pure rule tests for classify_features
    // -----------------------------------------------------------------------

    #[test]
    fn flat_low_gradient_low_variance() {
        let p = default_params();
        assert_eq!(
            classify_features(0.01, 0.0005, &p),
            RegionClass::Flat,
        );
    }

    #[test]
    fn textured_moderate_gradient() {
        let p = default_params();
        // gradient >= gradient_low (0.05), variance below variance_low
        assert_eq!(
            classify_features(0.10, 0.0005, &p),
            RegionClass::Textured,
        );
    }

    #[test]
    fn textured_moderate_variance() {
        let p = default_params();
        // gradient < gradient_low, variance >= variance_low (0.001)
        assert_eq!(
            classify_features(0.01, 0.005, &p),
            RegionClass::Textured,
        );
    }

    #[test]
    fn strong_edge_high_gradient_low_variance() {
        let p = default_params();
        assert_eq!(
            classify_features(0.50, 0.005, &p),
            RegionClass::StrongEdge,
        );
    }

    #[test]
    fn microtexture_high_variance_low_gradient() {
        let p = default_params();
        // variance >= variance_high (0.01) AND gradient < gradient_low (0.05)
        assert_eq!(
            classify_features(0.02, 0.015, &p),
            RegionClass::Microtexture,
        );
    }

    #[test]
    fn risky_halo_zone_high_gradient_high_variance() {
        let p = default_params();
        assert_eq!(
            classify_features(0.50, 0.015, &p),
            RegionClass::RiskyHaloZone,
        );
    }

    #[test]
    fn risky_halo_takes_priority_over_strong_edge() {
        let p = default_params();
        // Both gradient and variance are high -> RiskyHaloZone, not StrongEdge
        assert_eq!(
            classify_features(1.0, 0.1, &p),
            RegionClass::RiskyHaloZone,
        );
    }

    #[test]
    fn moderate_gradient_high_variance_is_textured_not_microtexture() {
        let p = default_params();
        // variance >= variance_high but gradient >= gradient_low (not < gradient_low)
        // so Microtexture rule does not match; falls through to Textured
        assert_eq!(
            classify_features(0.10, 0.015, &p),
            RegionClass::Textured,
        );
    }

    // -----------------------------------------------------------------------
    // gain_map_from_region_map
    // -----------------------------------------------------------------------

    #[test]
    fn gain_map_matches_table_lookup() {
        let map = RegionMap::new(2, 1, vec![
            RegionClass::Flat, RegionClass::StrongEdge,
        ]).unwrap();
        let gt = GainTable::v03_default();
        let gm = gain_map_from_region_map(&map, &gt);
        assert_eq!(gm.width, 2);
        assert_eq!(gm.height, 1);
        assert!((gm.get(0, 0) - 0.75).abs() < 1e-6);
        assert!((gm.get(1, 0) - 1.00).abs() < 1e-6);
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test -p r3sizer-core --lib classifier -- --nocapture`
Expected: PASS

Run: `cargo clippy --workspace -- -D warnings`
Expected: PASS (todo!() is allowed since it's a placeholder)

- [ ] **Step 3: Commit**

```bash
git add crates/r3sizer-core/src/classifier.rs crates/r3sizer-core/src/lib.rs
git commit -m "feat(classifier): classify_features rule, gain_map_from_region_map, tests"
```

---

### Task 7: `classifier.rs` — Sobel gradient and local variance

**Files:**
- Modify: `crates/r3sizer-core/src/classifier.rs`

- [ ] **Step 1: Write tests for Sobel gradient computation**

Add to the test module in `classifier.rs`:

```rust
    // -----------------------------------------------------------------------
    // Layer (b): feature extraction tests
    // -----------------------------------------------------------------------

    fn make_solid_image(w: u32, h: u32, value: f32) -> LinearRgbImage {
        LinearRgbImage::new(w, h, vec![value; (w * h * 3) as usize]).unwrap()
    }

    #[test]
    fn sobel_on_uniform_returns_zeros() {
        let luma = vec![0.5_f32; 8 * 8];
        let grad = sobel_gradient_magnitude(&luma, 8, 8);
        for &g in &grad {
            assert!(g.abs() < 1e-6, "expected ~0 gradient on uniform image, got {g}");
        }
    }

    #[test]
    fn sobel_on_vertical_edge_detects_edge() {
        let mut luma = vec![0.0_f32; 8 * 8];
        for y in 0..8_usize {
            for x in 4..8_usize {
                luma[y * 8 + x] = 1.0;
            }
        }
        let grad = sobel_gradient_magnitude(&luma, 8, 8);
        // Interior edge pixels near x=3..5 should have high gradient
        let edge_grad = grad[3 * 8 + 3]; // row 3, just left of edge
        assert!(edge_grad > 0.5, "expected significant gradient at edge, got {edge_grad}");
    }

    #[test]
    fn variance_on_uniform_returns_zeros() {
        let luma = vec![0.5_f32; 8 * 8];
        let var = local_variance(&luma, 8, 8, 5);
        for &v in &var {
            assert!(v.abs() < 1e-6, "expected ~0 variance on uniform image, got {v}");
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p r3sizer-core --lib classifier -- --nocapture`
Expected: FAIL — `sobel_gradient_magnitude` and `local_variance` not defined.

- [ ] **Step 3: Implement Sobel gradient and local variance**

Add to `classifier.rs` before the `classify` function:

```rust
// ---------------------------------------------------------------------------
// Pass 1: Sobel gradient magnitude (unnormalized, edge-replicate)
// ---------------------------------------------------------------------------

/// Unnormalized 3×3 Sobel gradient magnitude on single-channel data.
///
/// L2 norm: `g = sqrt(Gx² + Gy²)`. Theoretical max for luminance in [0,1]: 4√2 ≈ 5.66.
/// Border handling: edge-replicate (clamp pixel coordinates to valid range).
fn sobel_gradient_magnitude(luma: &[f32], width: usize, height: usize) -> Vec<f32> {
    let n = width * height;
    let mut grad = vec![0.0_f32; n];

    let clamp_x = |x: isize| -> usize { (x.max(0) as usize).min(width - 1) };
    let clamp_y = |y: isize| -> usize { (y.max(0) as usize).min(height - 1) };
    let px = |x: isize, y: isize| -> f32 { luma[clamp_y(y) * width + clamp_x(x)] };

    for y in 0..height {
        let yi = y as isize;
        for x in 0..width {
            let xi = x as isize;

            let gx = -px(xi - 1, yi - 1) + px(xi + 1, yi - 1)
                - 2.0 * px(xi - 1, yi) + 2.0 * px(xi + 1, yi)
                - px(xi - 1, yi + 1) + px(xi + 1, yi + 1);

            let gy = -px(xi - 1, yi - 1) - 2.0 * px(xi, yi - 1) - px(xi + 1, yi - 1)
                + px(xi - 1, yi + 1) + 2.0 * px(xi, yi + 1) + px(xi + 1, yi + 1);

            grad[y * width + x] = (gx * gx + gy * gy).sqrt();
        }
    }

    grad
}

// ---------------------------------------------------------------------------
// Pass 2: Local variance (edge-replicate)
// ---------------------------------------------------------------------------

/// Per-pixel variance of luminance in a square window.
///
/// `window_size` must be odd and >= 3 (validated by ClassificationParams).
/// Border handling: edge-replicate.
fn local_variance(luma: &[f32], width: usize, height: usize, window_size: usize) -> Vec<f32> {
    let n = width * height;
    let mut var = vec![0.0_f32; n];
    let half = (window_size / 2) as isize;
    let count = (window_size * window_size) as f32;

    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0_f32;
            let mut sum_sq = 0.0_f32;
            for dy in -half..=half {
                let yy = (y as isize + dy).max(0).min(height as isize - 1) as usize;
                for dx in -half..=half {
                    let xx = (x as isize + dx).max(0).min(width as isize - 1) as usize;
                    let v = luma[yy * width + xx];
                    sum += v;
                    sum_sq += v * v;
                }
            }
            let mean = sum / count;
            var[y * width + x] = (sum_sq / count - mean * mean).max(0.0);
        }
    }

    var
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p r3sizer-core --lib classifier -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/r3sizer-core/src/classifier.rs
git commit -m "feat(classifier): Sobel gradient magnitude and local variance passes"
```

---

### Task 8: `classifier.rs` — full `classify()` function

**Files:**
- Modify: `crates/r3sizer-core/src/classifier.rs`

- [ ] **Step 1: Write image-level classification tests**

Add to test module in `classifier.rs`:

```rust
    use crate::RegionCoverage;

    #[test]
    fn classify_solid_image_all_flat() {
        let img = make_solid_image(8, 8, 0.5);
        let params = default_params();
        let map = classify(&img, &params);
        assert_eq!(map.width, 8);
        assert_eq!(map.height, 8);
        let cov = RegionCoverage::from_region_map(&map);
        assert_eq!(cov.flat, 64);
        assert_eq!(cov.total_pixels, 64);
    }

    #[test]
    fn classify_step_edge_contains_strong_edge() {
        // Left half = 0.0, right half = 1.0
        let w = 16_u32;
        let h = 8_u32;
        let mut data = vec![0.0_f32; (w * h * 3) as usize];
        for y in 0..h {
            for x in (w / 2)..w {
                let idx = ((y * w + x) * 3) as usize;
                data[idx] = 1.0;
                data[idx + 1] = 1.0;
                data[idx + 2] = 1.0;
            }
        }
        let img = LinearRgbImage::new(w, h, data).unwrap();
        let map = classify(&img, &default_params());
        let cov = RegionCoverage::from_region_map(&map);
        assert!(cov.strong_edge > 0 || cov.risky_halo_zone > 0,
            "expected some StrongEdge or RiskyHaloZone pixels at the step edge");
        assert_eq!(
            cov.flat + cov.textured + cov.strong_edge + cov.microtexture + cov.risky_halo_zone,
            cov.total_pixels,
        );
    }

    #[test]
    fn classify_border_shapes_no_panic() {
        let p = default_params();
        // 1x1
        let img = make_solid_image(1, 1, 0.5);
        let _ = classify(&img, &p);
        // 1xN
        let img = make_solid_image(1, 8, 0.5);
        let _ = classify(&img, &p);
        // Nx1
        let img = make_solid_image(8, 1, 0.5);
        let _ = classify(&img, &p);
        // 2x2
        let img = make_solid_image(2, 2, 0.5);
        let _ = classify(&img, &p);
    }

    #[test]
    fn classify_is_deterministic() {
        let img = make_solid_image(8, 8, 0.5);
        let p = default_params();
        let map1 = classify(&img, &p);
        let map2 = classify(&img, &p);
        assert_eq!(map1.data(), map2.data());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p r3sizer-core --lib classifier -- --nocapture`
Expected: FAIL — `classify()` still has `todo!()`.

- [ ] **Step 3: Replace the classify() stub with the full implementation**

Replace the `classify` function body in `classifier.rs`:

```rust
pub fn classify(
    image: &LinearRgbImage,
    params: &ClassificationParams,
) -> RegionMap {
    let w = image.width() as usize;
    let h = image.height() as usize;

    // Pass 0: luminance extraction
    let luma: Vec<f32> = image
        .pixels()
        .chunks_exact(3)
        .map(|rgb| luminance(rgb[0], rgb[1], rgb[2]))
        .collect();

    // Pass 1: Sobel gradient magnitude
    let grad = sobel_gradient_magnitude(&luma, w, h);

    // Pass 2: local variance
    let var = local_variance(&luma, w, h, params.variance_window);

    // Pass 3: per-pixel classification
    let data: Vec<RegionClass> = grad
        .iter()
        .zip(var.iter())
        .map(|(&g, &v)| classify_features(g, v, params))
        .collect();

    RegionMap::new(image.width(), image.height(), data).unwrap()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p r3sizer-core --lib classifier -- --nocapture`
Expected: PASS

Run: `cargo clippy --workspace -- -D warnings`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/r3sizer-core/src/classifier.rs
git commit -m "feat(classifier): full classify() implementation — 4-pass luminance/gradient/variance/rule"
```

---

### Task 9: `sharpen.rs` — `adaptive_sharpen_lightness` and `adaptive_sharpen_rgb`

**Files:**
- Modify: `crates/r3sizer-core/src/sharpen.rs`

- [ ] **Step 1: Write tests for adaptive_sharpen**

Add to the existing `tests` module in `sharpen.rs`:

```rust
    use crate::{GainMap, CoreError};

    fn make_gain_map(w: u32, h: u32, value: f32) -> GainMap {
        GainMap::new(w, h, vec![value; (w * h) as usize]).unwrap()
    }

    #[test]
    fn adaptive_lightness_gain_one_matches_uniform() {
        let src = gradient(16, 16);
        let luma: Vec<f32> = src.pixels().chunks_exact(3)
            .map(|rgb| 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2])
            .collect();
        let gain_map = make_gain_map(16, 16, 1.0);
        let adaptive = adaptive_sharpen_lightness(&src, &luma, 1.5, &gain_map, 1.0).unwrap();
        let uniform = unsharp_mask_single_channel(
            &luma, 16, 16, 1.5, 1.0,
        ).unwrap();
        // Both should produce the same sharpened luminance → same RGB reconstruction
        let uniform_img = crate::color::reconstruct_rgb_from_lightness(&src, &uniform);
        for (a, b) in adaptive.pixels().iter().zip(uniform_img.pixels().iter()) {
            assert_abs_diff_eq!(a, b, epsilon = 1e-4);
        }
    }

    #[test]
    fn adaptive_lightness_gain_zero_is_identity() {
        let src = gradient(16, 16);
        let luma: Vec<f32> = src.pixels().chunks_exact(3)
            .map(|rgb| 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2])
            .collect();
        let gain_map = make_gain_map(16, 16, 0.0);
        let result = adaptive_sharpen_lightness(&src, &luma, 2.0, &gain_map, 1.0).unwrap();
        for (a, b) in src.pixels().iter().zip(result.pixels().iter()) {
            assert_abs_diff_eq!(a, b, epsilon = 1e-5);
        }
    }

    #[test]
    fn adaptive_rgb_gain_one_matches_uniform() {
        let src = gradient(16, 16);
        let gain_map = make_gain_map(16, 16, 1.0);
        let adaptive = adaptive_sharpen_rgb(&src, 1.5, &gain_map, 1.0).unwrap();
        let uniform = unsharp_mask(&src, 1.5, 1.0).unwrap();
        for (a, b) in adaptive.pixels().iter().zip(uniform.pixels().iter()) {
            assert_abs_diff_eq!(a, b, epsilon = 1e-4);
        }
    }

    #[test]
    fn adaptive_rgb_gain_zero_is_identity() {
        let src = gradient(16, 16);
        let gain_map = make_gain_map(16, 16, 0.0);
        let result = adaptive_sharpen_rgb(&src, 2.0, &gain_map, 1.0).unwrap();
        for (a, b) in src.pixels().iter().zip(result.pixels().iter()) {
            assert_abs_diff_eq!(a, b, epsilon = 1e-5);
        }
    }

    #[test]
    fn adaptive_preserves_out_of_range_values() {
        // Hard edge → large amount → ringing expected
        let mut data = vec![0.0f32; 32 * 1 * 3];
        for x in 16..32_usize {
            data[x * 3] = 1.0;
            data[x * 3 + 1] = 1.0;
            data[x * 3 + 2] = 1.0;
        }
        let src = LinearRgbImage::new(32, 1, data).unwrap();
        let gain_map = make_gain_map(32, 1, 1.5);
        let out = adaptive_sharpen_rgb(&src, 5.0, &gain_map, 1.0).unwrap();
        let has_oob = out.pixels().iter().any(|&v| v < 0.0 || v > 1.0);
        assert!(has_oob, "expected out-of-range values for strong adaptive sharpening");
    }

    #[test]
    fn adaptive_output_dimensions_match() {
        let src = gradient(16, 12);
        let gain_map = make_gain_map(16, 12, 1.0);
        let out = adaptive_sharpen_rgb(&src, 1.0, &gain_map, 1.0).unwrap();
        assert_eq!(out.width(), 16);
        assert_eq!(out.height(), 12);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p r3sizer-core --lib adaptive -- --nocapture`
Expected: FAIL — functions not defined.

- [ ] **Step 3: Implement adaptive_sharpen_lightness and adaptive_sharpen_rgb**

Add to `sharpen.rs` after the existing public functions, before the `tests` module:

```rust
// ---------------------------------------------------------------------------
// Adaptive sharpening (v0.3)
// ---------------------------------------------------------------------------

use crate::GainMap;

/// Adaptive unsharp mask on the lightness channel with per-pixel gain.
///
/// Computes blur once, then applies `L'(x,y) = L(x,y) + strength * gain(x,y) * D(x,y)`
/// where `D = L - blur(L)`. Reconstructs RGB via `k = L'/L`.
///
/// **No clamping.** Out-of-range values are the artifact signal.
///
/// # Detail buffer reuse
/// Returns `(image, detail_buffer)` where `detail_buffer = L - blur(L)`.
/// For backoff iterations with a different strength, the caller can call
/// [`apply_adaptive_lightness_from_detail`] to avoid recomputing the blur.
pub fn adaptive_sharpen_lightness(
    src: &LinearRgbImage,
    luminance: &[f32],
    strength: f32,
    gain_map: &GainMap,
    sigma: f32,
) -> Result<LinearRgbImage, CoreError> {
    debug_assert_eq!(luminance.len(), (src.width() as usize) * (src.height() as usize));
    debug_assert_eq!(gain_map.width, src.width());
    debug_assert_eq!(gain_map.height, src.height());

    let kernel = make_kernel(sigma)?;
    let w = src.width() as usize;
    let h = src.height() as usize;

    let blurred = gaussian_blur_single_channel(luminance, w, h, &kernel);

    // Detail layer: D = L - blur(L)
    let detail: Vec<f32> = luminance.iter().zip(blurred.iter())
        .map(|(&l, &b)| l - b)
        .collect();

    let sharpened_l = apply_adaptive_lightness_from_detail(luminance, &detail, strength, gain_map);
    Ok(crate::color::reconstruct_rgb_from_lightness(src, &sharpened_l))
}

/// Apply adaptive sharpening from pre-computed detail buffer.
///
/// `L'(x,y) = L(x,y) + strength * gain(x,y) * detail(x,y)`
///
/// Used by the backoff loop to avoid recomputing the Gaussian blur.
pub fn apply_adaptive_lightness_from_detail(
    luminance: &[f32],
    detail: &[f32],
    strength: f32,
    gain_map: &GainMap,
) -> Vec<f32> {
    let gain_data = gain_map.data();
    luminance.iter().zip(detail.iter()).zip(gain_data.iter())
        .map(|((&l, &d), &g)| l + strength * g * d)
        .collect()
}

/// Adaptive unsharp mask on RGB channels with per-pixel gain.
///
/// Computes blur once per channel, then applies
/// `C'(x,y) = C(x,y) + strength * gain(x,y) * (C(x,y) - blur_C(x,y))`
///
/// **No clamping.**
pub fn adaptive_sharpen_rgb(
    src: &LinearRgbImage,
    strength: f32,
    gain_map: &GainMap,
    sigma: f32,
) -> Result<LinearRgbImage, CoreError> {
    debug_assert_eq!(gain_map.width, src.width());
    debug_assert_eq!(gain_map.height, src.height());

    let kernel = make_kernel(sigma)?;
    let blurred = gaussian_blur(src, &kernel);

    let src_px = src.pixels();
    let blur_px = blurred.pixels();
    let gain_data = gain_map.data();
    let npixels = (src.width() as usize) * (src.height() as usize);

    let mut out = Vec::with_capacity(npixels * 3);
    for i in 0..npixels {
        let g = gain_data[i];
        let eff = strength * g;
        let base = i * 3;
        out.push(src_px[base] + eff * (src_px[base] - blur_px[base]));
        out.push(src_px[base + 1] + eff * (src_px[base + 1] - blur_px[base + 1]));
        out.push(src_px[base + 2] + eff * (src_px[base + 2] - blur_px[base + 2]));
    }

    LinearRgbImage::new(src.width(), src.height(), out)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p r3sizer-core --lib -- adaptive --nocapture`
Expected: PASS

Run: `cargo clippy --workspace -- -D warnings`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/r3sizer-core/src/sharpen.rs
git commit -m "feat(sharpen): adaptive_sharpen_lightness/rgb with per-pixel gain and detail buffer reuse"
```

---

### Task 10: Pipeline integration — Stage 2.5 classification + adaptive final sharpen

**Files:**
- Modify: `crates/r3sizer-core/src/pipeline.rs`

- [ ] **Step 1: Add imports for new types and classifier**

At the top of `pipeline.rs`, add to imports:

```rust
use crate::{
    classifier::{classify, gain_map_from_region_map},
    // ... existing imports ...
    AdaptiveValidationOutcome, GainMap, RegionCoverage, SharpenStrategy,
};
use crate::sharpen::{
    adaptive_sharpen_lightness, adaptive_sharpen_rgb,
    apply_adaptive_lightness_from_detail,
};
```

- [ ] **Step 2: Add Stage 2.5 — classification after contrast leveling**

In `process_auto_sharp_downscale`, after the contrast leveling block (Stage 3, around line 76), add:

```rust
    // -------------------------------------------------------------------
    // 2.5. Region classification (ContentAdaptive only)
    // -------------------------------------------------------------------
    let t0 = Instant::now();
    let (region_map, gain_map, region_coverage, classification_us) =
        match &params.sharpen_strategy {
            SharpenStrategy::ContentAdaptive { classification, gain_table, .. } => {
                let rmap = classify(&base, classification);
                let gmap = gain_map_from_region_map(&rmap, gain_table);
                let cov = RegionCoverage::from_region_map(&rmap);
                let us = t0.elapsed().as_micros() as u64;
                (Some(rmap), Some(gmap), Some(cov), Some(us))
            }
            SharpenStrategy::Uniform => (None, None, None, None),
        };
```

- [ ] **Step 3: Replace Stage 8 final sharpening with strategy dispatch**

Replace the "8. Final sharpening" block (around line 238-248) with:

```rust
    // -------------------------------------------------------------------
    // 8. Final sharpening (strategy-dependent)
    // -------------------------------------------------------------------
    let t0 = Instant::now();
    let selected_strength = solve_result.selected_strength;

    let (mut final_image, adaptive_validation, adaptive_validation_us) =
        match (&params.sharpen_strategy, &gain_map) {
            (SharpenStrategy::Uniform, _) | (_, None) => {
                let result = sharpen_image(
                    &base, base_luminance.as_deref(),
                    params.sharpen_mode, params.sharpen_model,
                    selected_strength, &kernel,
                )?;
                (result.image, None, None)
            }
            (
                SharpenStrategy::ContentAdaptive {
                    max_backoff_iterations,
                    backoff_scale_factor,
                    ..
                },
                Some(gm),
            ) => {
                adaptive_sharpen_with_validation(
                    &base,
                    base_luminance.as_deref(),
                    params.sharpen_mode,
                    selected_strength,
                    gm,
                    params.sharpen_sigma,
                    params.target_artifact_ratio,
                    params.artifact_metric,
                    params.metric_mode,
                    baseline_artifact_ratio,
                    *max_backoff_iterations,
                    *backoff_scale_factor,
                    &kernel,
                )?
            }
        };
    let final_sharpen_us = t0.elapsed().as_micros() as u64;
```

- [ ] **Step 4: Add the `adaptive_sharpen_with_validation` helper**

Add this helper function to `pipeline.rs`:

```rust
/// Adaptive sharpen + validate + backoff (Stage 9 + 9.5).
///
/// Returns `(final_image, validation_outcome, validation_time_us)`.
#[allow(clippy::too_many_arguments)]
fn adaptive_sharpen_with_validation(
    base: &LinearRgbImage,
    base_luminance: Option<&[f32]>,
    sharpen_mode: SharpenMode,
    global_strength: f32,
    gain_map: &GainMap,
    sigma: f32,
    target_p0: f32,
    artifact_metric: ArtifactMetric,
    metric_mode: MetricMode,
    baseline_artifact_ratio: f32,
    max_backoff: u8,
    backoff_factor: f32,
    kernel: &[f32],
) -> Result<(LinearRgbImage, Option<AdaptiveValidationOutcome>, Option<u64>), CoreError> {
    let w = base.width() as usize;
    let h = base.height() as usize;

    let measure = |img: &LinearRgbImage| -> f32 {
        let raw = match artifact_metric {
            ArtifactMetric::ChannelClippingRatio => channel_clipping_ratio(img),
            ArtifactMetric::PixelOutOfGamutRatio => crate::metrics::pixel_out_of_gamut_ratio(img),
        };
        compute_metric_value(raw, baseline_artifact_ratio, metric_mode)
    };

    // Compute detail buffer once.
    match sharpen_mode {
        SharpenMode::Lightness => {
            let luma = base_luminance.expect("luminance required for lightness mode");
            let blur_l = crate::sharpen::gaussian_blur_single_channel_with_kernel(
                luma, w, h, kernel,
            );
            let detail: Vec<f32> = luma.iter().zip(blur_l.iter())
                .map(|(&l, &b)| l - b).collect();

            let apply_at_scale = |scale: f32| -> LinearRgbImage {
                let sharpened_l = crate::sharpen::apply_adaptive_lightness_from_detail(
                    luma, &detail, global_strength * scale, gain_map,
                );
                crate::color::reconstruct_rgb_from_lightness(base, &sharpened_l)
            };

            // Initial apply at scale=1.0
            let mut result = apply_at_scale(1.0);
            let mut p = measure(&result);

            let t_val = Instant::now();

            if p <= target_p0 {
                let val_us = t_val.elapsed().as_micros() as u64;
                return Ok((
                    result,
                    Some(AdaptiveValidationOutcome::PassedDirect { measured_metric: p }),
                    Some(val_us),
                ));
            }

            // Backoff loop
            let mut best_scale = 1.0_f32;
            let mut best_metric = p;
            let mut best_result = result;
            let mut scale = 1.0_f32;

            for i in 1..=max_backoff {
                scale *= backoff_factor;
                result = apply_at_scale(scale);
                p = measure(&result);

                if p < best_metric {
                    best_metric = p;
                    best_scale = scale;
                    best_result = result.clone();
                }

                if p <= target_p0 {
                    let val_us = t_val.elapsed().as_micros() as u64;
                    return Ok((
                        best_result,
                        Some(AdaptiveValidationOutcome::PassedAfterBackoff {
                            iterations: i,
                            final_scale: scale,
                            measured_metric: p,
                        }),
                        Some(val_us),
                    ));
                }
            }

            let val_us = t_val.elapsed().as_micros() as u64;
            Ok((
                best_result,
                Some(AdaptiveValidationOutcome::FailedBudgetExceeded {
                    iterations: max_backoff,
                    best_scale,
                    best_metric,
                }),
                Some(val_us),
            ))
        }

        SharpenMode::Rgb => {
            // RGB mode: compute blur once, apply with varying scale
            let blurred = crate::sharpen::gaussian_blur_rgb_with_kernel(base, kernel);
            let src_px = base.pixels();
            let blur_px = blurred.pixels();
            let gain_data = gain_map.data();
            let npixels = w * h;

            let apply_at_scale = |scale: f32| -> LinearRgbImage {
                let eff_strength = global_strength * scale;
                let mut out = Vec::with_capacity(npixels * 3);
                for i in 0..npixels {
                    let eff = eff_strength * gain_data[i];
                    let b3 = i * 3;
                    out.push(src_px[b3] + eff * (src_px[b3] - blur_px[b3]));
                    out.push(src_px[b3 + 1] + eff * (src_px[b3 + 1] - blur_px[b3 + 1]));
                    out.push(src_px[b3 + 2] + eff * (src_px[b3 + 2] - blur_px[b3 + 2]));
                }
                LinearRgbImage::new(base.width(), base.height(), out).unwrap()
            };

            let mut result = apply_at_scale(1.0);
            let mut p = measure(&result);

            let t_val = Instant::now();

            if p <= target_p0 {
                let val_us = t_val.elapsed().as_micros() as u64;
                return Ok((
                    result,
                    Some(AdaptiveValidationOutcome::PassedDirect { measured_metric: p }),
                    Some(val_us),
                ));
            }

            let mut best_scale = 1.0_f32;
            let mut best_metric = p;
            let mut best_result = result;
            let mut scale = 1.0_f32;

            for i in 1..=max_backoff {
                scale *= backoff_factor;
                result = apply_at_scale(scale);
                p = measure(&result);

                if p < best_metric {
                    best_metric = p;
                    best_scale = scale;
                    best_result = result.clone();
                }

                if p <= target_p0 {
                    let val_us = t_val.elapsed().as_micros() as u64;
                    return Ok((
                        best_result,
                        Some(AdaptiveValidationOutcome::PassedAfterBackoff {
                            iterations: i,
                            final_scale: scale,
                            measured_metric: p,
                        }),
                        Some(val_us),
                    ));
                }
            }

            let val_us = t_val.elapsed().as_micros() as u64;
            Ok((
                best_result,
                Some(AdaptiveValidationOutcome::FailedBudgetExceeded {
                    iterations: max_backoff,
                    best_scale,
                    best_metric,
                }),
                Some(val_us),
            ))
        }
    }
}
```

- [ ] **Step 5: Expose `gaussian_blur_single_channel_with_kernel` and `gaussian_blur` as `pub(crate)` in sharpen.rs**

In `sharpen.rs`, change the visibility of the functions the pipeline needs:

- `gaussian_blur_single_channel` → rename usage to use the existing `gaussian_blur_single_channel_with_kernel` pattern. Actually, we need to expose the blur function that takes a kernel. Add a new public wrapper:

```rust
/// Expose single-channel blur with pre-built kernel for pipeline reuse.
pub(crate) fn gaussian_blur_single_channel_with_kernel_raw(
    data: &[f32],
    width: usize,
    height: usize,
    kernel: &[f32],
) -> Vec<f32> {
    gaussian_blur_single_channel(data, width, height, kernel)
}

/// Expose RGB blur with pre-built kernel for pipeline reuse.
pub(crate) fn gaussian_blur_rgb_with_kernel(
    src: &LinearRgbImage,
    kernel: &[f32],
) -> LinearRgbImage {
    gaussian_blur(src, kernel)
}
```

Update the pipeline to use `crate::sharpen::gaussian_blur_single_channel_with_kernel_raw` in place of `gaussian_blur_single_channel_with_kernel`.

Actually, looking at the existing code, `gaussian_blur_single_channel` is already private and called by `unsharp_mask_single_channel_with_kernel`. The simplest approach: make `gaussian_blur_single_channel` `pub(crate)` directly:

Change `fn gaussian_blur_single_channel(` to `pub(crate) fn gaussian_blur_single_channel(`.

Change `fn gaussian_blur(` to `pub(crate) fn gaussian_blur(`.

- [ ] **Step 6: Update diagnostics assembly to include new fields**

Update the diagnostics construction in the return block to use the computed values:

```rust
        region_coverage,
        adaptive_validation,
```

And the timing block:

```rust
            classification_us,
            adaptive_validation_us,
```

- [ ] **Step 7: Remove the separate "9. Measure actual artifact ratio" block**

For the Uniform path, the existing final measurement code stays. For ContentAdaptive, the measurement is done inside `adaptive_sharpen_with_validation`. Adjust the code after the strategy dispatch so the measurement only happens for the Uniform path (the ContentAdaptive path already measured inside the helper).

The simplest approach: keep the measurement block but run it unconditionally on `final_image` — this ensures diagnostics are consistent. The measurement is cheap compared to the sharpen.

- [ ] **Step 8: Run full test suite**

Run: `cargo test --workspace`
Expected: PASS

Run: `cargo clippy --workspace -- -D warnings`
Expected: PASS

- [ ] **Step 9: Commit**

```bash
git add crates/r3sizer-core/src/pipeline.rs crates/r3sizer-core/src/sharpen.rs
git commit -m "feat(pipeline): integrate adaptive sharpening — classification, adaptive final, validation/backoff"
```

---

### Task 11: Integration tests — Uniform regression guard

**Files:**
- Modify: `crates/r3sizer-core/tests/integration.rs`

- [ ] **Step 1: Add Uniform regression test**

Add to `integration.rs`:

```rust
// ---------------------------------------------------------------------------
// Content-adaptive sharpening (v0.3)
// ---------------------------------------------------------------------------

#[test]
fn uniform_strategy_identical_to_default() {
    // SharpenStrategy::Uniform is the default — verify existing behaviour is unchanged.
    let src = gradient_image(64, 64);
    let params = default_params(16, 16);
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let d = &out.diagnostics;

    // New fields are None for Uniform
    assert!(d.region_coverage.is_none(), "region_coverage should be None for Uniform");
    assert!(d.adaptive_validation.is_none(), "adaptive_validation should be None for Uniform");
    assert!(d.timing.classification_us.is_none());
    assert!(d.timing.adaptive_validation_us.is_none());

    // Existing semantics unchanged
    assert!(d.selected_strength > 0.0);
    assert!(d.measured_artifact_ratio.is_finite());
    assert!(d.measured_metric_value.is_finite());
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p r3sizer-core --test integration uniform_strategy -- --nocapture`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/r3sizer-core/tests/integration.rs
git commit -m "test(integration): Uniform strategy regression guard for v0.3"
```

---

### Task 12: Integration tests — ContentAdaptive happy path + backoff

**Files:**
- Modify: `crates/r3sizer-core/tests/integration.rs`

- [ ] **Step 1: Add imports for new types**

Add to the import block at the top of `integration.rs`:

```rust
use r3sizer_core::{
    AdaptiveValidationOutcome, ClassificationParams, GainTable, RegionCoverage, SharpenStrategy,
    // ... existing imports ...
};
```

- [ ] **Step 2: Add ContentAdaptive happy path test**

```rust
#[test]
fn content_adaptive_happy_path() {
    let src = gradient_image(64, 64);
    let params = AutoSharpParams {
        target_artifact_ratio: 0.1, // generous P0
        sharpen_strategy: SharpenStrategy::ContentAdaptive {
            classification: ClassificationParams::default(),
            gain_table: GainTable::v03_default(),
            max_backoff_iterations: 4,
            backoff_scale_factor: 0.8,
        },
        ..default_params(16, 16)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let d = &out.diagnostics;

    // Region coverage present and sums to pixel count
    let cov = d.region_coverage.as_ref().expect("region_coverage should be Some");
    assert_eq!(cov.total_pixels, 16 * 16);
    assert_eq!(
        cov.flat + cov.textured + cov.strong_edge + cov.microtexture + cov.risky_halo_zone,
        cov.total_pixels,
    );

    // Adaptive validation present
    let val = d.adaptive_validation.as_ref().expect("adaptive_validation should be Some");
    match val {
        AdaptiveValidationOutcome::PassedDirect { measured_metric } => {
            assert!(*measured_metric <= 0.1);
        }
        AdaptiveValidationOutcome::PassedAfterBackoff { measured_metric, .. } => {
            assert!(*measured_metric <= 0.1);
        }
        _ => {} // FailedBudgetExceeded is acceptable — it's content-dependent
    }

    // Timing fields populated
    assert!(d.timing.classification_us.is_some());
    assert!(d.timing.adaptive_validation_us.is_some());

    // Output is valid
    assert_eq!(out.image.width(), 16);
    assert_eq!(out.image.height(), 16);
    for &v in out.image.pixels() {
        assert!(v >= 0.0 && v <= 1.0, "pixel {v} outside [0,1] after clamping");
    }
}
```

- [ ] **Step 3: Add determinism test**

```rust
#[test]
fn content_adaptive_deterministic() {
    let src = gradient_image(64, 64);
    let params = AutoSharpParams {
        sharpen_strategy: SharpenStrategy::ContentAdaptive {
            classification: ClassificationParams::default(),
            gain_table: GainTable::v03_default(),
            max_backoff_iterations: 4,
            backoff_scale_factor: 0.8,
        },
        ..default_params(16, 16)
    };
    let out1 = process_auto_sharp_downscale(&src, &params).unwrap();
    let out2 = process_auto_sharp_downscale(&src, &params).unwrap();
    assert_eq!(out1.image.pixels(), out2.image.pixels(), "adaptive pipeline must be deterministic");
    assert_eq!(
        out1.diagnostics.selected_strength,
        out2.diagnostics.selected_strength,
    );
}
```

- [ ] **Step 4: Add backoff / budget exceeded test**

```rust
#[test]
fn content_adaptive_tight_budget_triggers_backoff_or_failure() {
    // Use a checkerboard (lots of edges) with very tight P0 and boosted gains
    let src = checkerboard(32, 32);
    let params = AutoSharpParams {
        target_artifact_ratio: 0.0001, // very tight
        sharpen_strategy: SharpenStrategy::ContentAdaptive {
            classification: ClassificationParams::default(),
            gain_table: GainTable::new(1.5, 1.5, 2.0, 2.0, 1.5).unwrap(), // boosted gains
            max_backoff_iterations: 4,
            backoff_scale_factor: 0.8,
        },
        ..default_params(8, 8)
    };
    let out = process_auto_sharp_downscale(&src, &params).unwrap();
    let val = out.diagnostics.adaptive_validation.as_ref()
        .expect("adaptive_validation should be Some");

    // With tight budget and boosted gains, we expect either backoff or failure
    match val {
        AdaptiveValidationOutcome::PassedDirect { .. } => {
            // This is possible but unlikely with these params
        }
        AdaptiveValidationOutcome::PassedAfterBackoff { iterations, .. } => {
            assert!(*iterations > 0);
        }
        AdaptiveValidationOutcome::FailedBudgetExceeded { iterations, .. } => {
            assert!(*iterations > 0);
        }
    }
}
```

- [ ] **Step 5: Run all integration tests**

Run: `cargo test -p r3sizer-core --test integration -- --nocapture`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/r3sizer-core/tests/integration.rs
git commit -m "test(integration): ContentAdaptive happy path, determinism, backoff tests"
```

---

### Task 13: Final verification — clippy + full test suite

**Files:** None (verification only)

- [ ] **Step 1: Run full test suite**

Run: `cargo test --workspace`
Expected: PASS — all tests pass.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`
Expected: PASS — no warnings.

- [ ] **Step 3: Run a quick functional check with the CLI**

Run: `cargo run -p r3sizer-cli -- --help`
Expected: Prints help. New `sharpen_strategy` is not yet exposed as CLI flag (out of scope).

- [ ] **Step 4: Verify JSON diagnostics round-trip still works**

The existing `diagnostics_json_round_trip` integration test covers this. Verify it passes by checking test output from Step 1.

- [ ] **Step 5: No commit needed — verification only**

---
