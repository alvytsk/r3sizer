# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```sh
# Build
cargo build --workspace

# Test all
cargo test --workspace

# Test a single test by name
cargo test -p r3sizer-core <test_name>

# Run integration tests only
cargo test -p r3sizer-core --test integration

# Lint (warnings are errors)
cargo clippy --workspace -- -D warnings

# Run benchmarks
cargo bench -p r3sizer-core

# Run the CLI (single file)
cargo run -p r3sizer -- process --input <FILE> --output <FILE> --width <N> --height <N>
cargo run -p r3sizer -- process -i photo.jpg -o out.png --width 800 --height 600 --diagnostics diag.json

# Run the CLI (sweep mode)
cargo run -p r3sizer -- sweep --in-dir ./photos --out-dir ./out --summary summary.json --width 800 --height 600

# Regenerate TypeScript types from Rust (after changing types.rs)
cargo test -p r3sizer-core --features typegen export_typescript_bindings -- --nocapture
```

## Architecture

Four crates with a strict dependency direction: `r3sizer-core` ← `r3sizer-io` ← `r3sizer`, and `r3sizer-core` ← `r3sizer-wasm`.

**`r3sizer-core`** — all image processing logic, no I/O. Reusable in CLI, WASM, and future Tauri GUI. Modules map 1:1 to pipeline stages:

- `types.rs` — all shared data types (`LinearRgbImage`, `AutoSharpParams`, `ProcessOutput`, `AutoSharpDiagnostics`, `CubicPolynomial`, `ProbeSample`, `SharpenMode`, `MetricMode`, `ArtifactMetric`, `FitStatus`, `FitQuality`, `CrossingStatus`, `SelectionMode`, `FallbackReason`, `RobustnessFlags`, `StageTiming`, `MetricBreakdown`, `MetricComponent`, `SharpenStrategy`, `ResizeStrategy`, `ProbeConfig`, `PipelineMode`, etc.)
- `color.rs` — sRGB ↔ linear RGB (IEC 61966-2-1), CIE Y luminance extraction, lightness-based RGB reconstruction
- `resize.rs` — Lanczos3 downscale via `fast_image_resize` (SIMD-accelerated on x86-64); staged bilinear pre-reduce for shrink ratios ≥ 3×
- `resize_strategy.rs` — content-adaptive resize (uniform or per-region kernel selection)
- `sharpen.rs` — unsharp mask (3-channel RGB and single-channel luminance) with hand-rolled separable Gaussian; **deliberately no clamping** so out-of-range values exist for the metric
- `metrics/` — sub-modules for gamut, halo, overshoot, texture, edges, composite; `channel_clipping_ratio` and `pixel_out_of_gamut_ratio` selectable via `ArtifactMetric`; `compute_metric_breakdown` produces component-wise `MetricBreakdown`
- `classifier.rs` — Sobel gradient + local variance → 5-class region map (Flat, Textured, StrongEdge, Microtexture, RiskyHaloZone)
- `fit.rs` — 4×4 Vandermonde normal equations solved by Gaussian elimination with partial pivoting, all in **f64**; `fit_cubic_with_quality` returns `FitQuality` (R², residuals, min pivot); `check_monotonicity` validates probe sample ordering
- `solve.rs` — Cardano's formula for cubic roots + fallback to best probe sample; returns `SolveResult` with `SelectionMode` and `CrossingStatus`
- `contrast.rs` — placeholder contrast leveling (percentile stretch); real formula unknown
- `chroma_guard.rs` — soft chroma clamping with context-aware thresholds per region
- `base_quality.rs` — resize quality scoring (edge retention, texture retention, ringing); `full_diagnostics` flag skips expensive source-side metrics in fast/balanced modes
- `evaluator.rs` — heuristic quality evaluator (feature extraction + advisory strength cap)
- `recommendations.rs` — diagnostic-driven parameter suggestions
- `pipeline.rs` — orchestrates all stages in a **two-phase architecture**: `prepare_base` (resize, classify, baseline — cached as `PreparedBase`) and `process_from_prepared` (probing, fit, solve, sharpen). Also exposes `resolve_initial_strengths` and `resolve_dense_strengths` for JS-side TwoPass parallel probing, and `compute_probe_detail` / `run_probes_from_detail` for parallel probe workers that receive precomputed detail signal. Early stopping in coarse probing exits as soon as a P0 bracket is found. Detail precomputation (`D = input - blur(input)`) computes the Gaussian blur once and shares the result across coarse and dense phases. `PreparedBase` carries a `BaseParamsKey` fingerprint so cache reuse is safe across param changes. One-shot entry point `process_auto_sharp_downscale` remains for CLI use.

**`r3sizer-io`** — `load_as_linear` (file → `LinearRgbImage`, applies sRGB→linear) and `save_from_linear` (applies linear→sRGB, writes file). Format inferred from extension.

**`r3sizer`** (CLI) — thin wrapper: `args.rs` (clap), `run.rs` (load→process→save), `output.rs` (stdout formatting), `sweep.rs` (batch directory processing with aggregate statistics).

**`r3sizer-wasm`** — WebAssembly bindings (`wasm-bindgen`). WASM exports: `prepare_image` (sRGB→linear cache), `prepare_base` (resize+classify+baseline cache with params fingerprint fast-path), `process_image` (full pipeline), `get_base_data` (extract cached base for probe workers), `probe_batch` (run probes on raw base data), `compute_probe_detail` (precompute detail signal `D = input - blur(input)` for probe workers), `probe_batch_with_detail` (run probes using precomputed detail, skipping per-probe blur), `process_from_probes` (finish pipeline with externally-collected probes), `resolve_initial_strengths` (coarse/all strengths for parallel probing), `resolve_dense_strengths` (TwoPass dense window from coarse results). Thread-local `RefCell` caching for input, `PreparedBase`, and precomputed detail signal. Depends on `r3sizer-core` with `default-features = false` (no rayon). Color conversion in `convert.rs`.

**`web/`** — React 19 + Vite + Tailwind diagnostic UI. Communicates with `r3sizer-wasm` via a main Web Worker (`wasm.ts` / `wasm-worker.ts`) and a **probe worker pool** (`probe-pool.ts` / `probe-worker.ts`) for parallel sharpening strength evaluation. The pool distributes probes across N workers (up to 6), supports TwoPass two-round probing, and caches base data in workers to avoid redundant structured clones. Probe workers receive precomputed detail signal and skip the Gaussian blur when detail is available, using `probe_batch_with_detail` instead of `probe_batch`. State managed by Zustand (`processor-store.ts`). TypeScript types are auto-generated from Rust via `ts-rs` (see below).

### TypeScript type generation (`ts-rs`)

All serializable types in `r3sizer-core/src/types.rs` have `#[cfg_attr(feature = "typegen", derive(TS))]`. Running `cargo test -p r3sizer-core --features typegen export_typescript_bindings` writes `web/src/types/generated.ts` containing all type definitions and serialized `Default` constants. The web app imports from `web/src/types/wasm-types.ts` which re-exports everything from `generated.ts` and adds WASM-specific types (`ProcessResult`) and web overrides (e.g. `diagnostics_level: "full"`). When changing types in Rust, regenerate and commit `generated.ts`.

## Deployment

The web UI is deployed to GitHub Pages at https://alvytsk.github.io/r3sizer/ via `.github/workflows/deploy.yml`. The workflow triggers on every push to `main` and runs: install Rust + wasm target → `wasm-pack` → `npm ci` → `npm run build` → upload + deploy to Pages. The build runs entirely in CI (no local artifacts needed).

## Key design decisions to preserve

- **f32 for pixels, f64 for polynomial fitting** — the Vandermonde matrix has terms up to `s^6`; f32 causes catastrophic cancellation.
- **No clamping inside `sharpen.rs`** — out-of-range values are the artifact signal. Clamping happens only in `pipeline.rs` at the final output stage.
- **`downscaled` image is never mutated during probing** — each probe in the loop produces a fresh allocation, leaving `base` unchanged for the final apply. In lightness mode, luminance is extracted once and reused across all probes.
- **Fallback is not an error** — when the cubic solve finds no root in the probe range, `solve.rs` falls back to the best probe sample. The pipeline always returns a result. Selection outcome is reported via `SelectionMode` enum (`PolynomialRoot`, `BestSampleWithinBudget`, `LeastBadSample`, `BudgetUnreachable`).
- **Lightness-based sharpening is the default** — `SharpenMode::Lightness` sharpens CIE Y luminance, then reconstructs RGB via `k = L'/L`. This is paper-supported (strong inference from paper, not yet explicitly confirmed). `SharpenMode::Rgb` is kept for comparison.
- **Baseline measurement separates resize from sharpen artifacts** — `MetricMode::RelativeToBase` (default) subtracts the pre-sharpen baseline from each probe measurement, so the fitted metric only reflects sharpening-induced artifacts.
- **`contrast.rs` is a documented stub** — `ContrastLevelingParams::enabled = false` by default. The function signature and placement are fixed; only the body needs replacement once the paper formula is known.
- **Robustness checks gate the polynomial root** — monotonicity, R² > 0.85, condition number (min pivot > 1e-8), and leave-one-out stability are checked after fitting. If any check fails, the pipeline falls back to direct search and records a typed `FallbackReason`.
- **Per-stage timing is always collected** — `StageTiming` records microsecond-resolution wall-clock time for each pipeline stage. Zero overhead when unused (no allocation, just `Instant::now()`).
- **All four composite metric components are active** — `MetricBreakdown` with `MetricComponent` variants (GamutExcursion, HaloRinging, EdgeOvershoot, TextureFlattening) is populated per probe. The `aggregate` field preserves backward compatibility with the scalar fitting path. Configurable weights via `MetricWeights` (default: 1.0, 0.3, 0.3, 0.1).
- **Two-phase pipeline for interactive use** — `prepare_base` computes resize + classify + baseline (~1.5s on 24MP) and caches as `PreparedBase`. `process_from_prepared` runs probing + fit + solve + sharpen (~0.5s). A `BaseParamsKey` fingerprint ensures the cache is only reused when all base-affecting params match.
- **Parallel probing in WASM** — probe workers receive base data once via `set_base`, then run batches without re-sending pixels. TwoPass probing is supported: coarse round in parallel → resolve dense window in Rust → dense round in parallel → merge → fit+sharpen. Graceful fallback to single-worker on any failure.
- **Content-adaptive sharpening is the default** — `SharpenStrategy::ContentAdaptive` classifies pixels into 5 regions and modulates sharpening strength via a per-pixel gain map. Backoff loop reduces strength if budget is exceeded.
- **Chroma guard is on by default** — soft chroma clamping after lightness-based sharpening, with context-aware thresholds modulated by region class and saturation.
- **TypeScript types are generated from Rust, not handwritten** — `ts-rs` with `serde-compat` reads `#[serde(...)]` attributes and produces matching TypeScript. The `typegen` feature flag keeps `ts-rs` out of production builds. Default constants are serialized from Rust `Default` impls so they stay in sync. `generated.ts` must be committed; the Docker build regenerates it to ensure freshness.
- **Detail signal precomputed once per probe phase** — `D = input - blur(input)` is independent of sharpening strength `s`. The Gaussian blur (dominant per-probe cost) runs once; each probe then applies `out = input + s × D` as a trivial multiply-add. This collapses probe cost from `N × blur` to `1 × blur + N × apply`.
- **Runtime modes control the speed-quality tradeoff** — `PipelineMode::Fast` uses fewer probes, uniform sharpening, no chroma guard; `Balanced` is the default; `Quality` extends probing and guarantees full adaptive pipeline. Applied via `AutoSharpParams::resolved()` before pipeline entry.

## Algorithm summary

The core idea: select sharpening strength by constraining the fraction of channel values that fall outside [0,1] after sharpening (artifact ratio P) to a target threshold P0 (default 0.3% for Photo preset). A cubic P(s) is fitted to probe measurements, then solved for s*.

```
linearize → downscale (with optional content-adaptive kernel) →
[contrast leveling] → classify regions → measure baseline P(base) →
extract luminance L → build gain map →
precompute detail signal D = L - blur(L) (one Gaussian blur) →
probe N strengths (two-pass adaptive: coarse scan with early stopping → dense refinement):
  { apply sharpened = L + s_i × D → reconstruct RGB → chroma guard → measure P(s_i) } →
fit cubic P_hat(s) (with quality metrics) →
robustness checks (monotonicity, LOO, R², condition) →
solve P_hat(s*) = P0 → adaptive backoff if budget exceeded →
final sharpen(s*) → chroma guard → clamp → output + recommendations
```

See `docs/algorithm.md`, `docs/assumptions.md`, and `docs/future_work.md` for details on confirmed vs. approximated paper details and the Tauri integration path.
