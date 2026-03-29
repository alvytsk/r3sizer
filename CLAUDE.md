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
cargo run -p r3sizer-cli -- --input <FILE> --output <FILE> --width <N> --height <N>
cargo run -p r3sizer-cli -- --input photo.jpg --output out.png --width 800 --height 600 --diagnostics diag.json

# Run the CLI (sweep mode)
cargo run -p r3sizer-cli -- --sweep-dir ./photos --sweep-output-dir ./out --sweep-summary summary.json --width 800 --height 600

# Regenerate TypeScript types from Rust (after changing types.rs)
cargo test -p r3sizer-core --features typegen export_typescript_bindings -- --nocapture
```

## Architecture

Four crates with a strict dependency direction: `r3sizer-core` ← `r3sizer-io` ← `r3sizer-cli`, and `r3sizer-core` ← `r3sizer-wasm`.

**`r3sizer-core`** — all image processing logic, no I/O. This is the library meant to be reused in a future Tauri GUI or WASM build. Modules map 1:1 to pipeline stages:

- `types.rs` — all shared data types (`LinearRgbImage`, `AutoSharpParams`, `ProcessOutput`, `AutoSharpDiagnostics`, `CubicPolynomial`, `ProbeSample`, `SharpenMode`, `SharpenModel`, `MetricMode`, `ArtifactMetric`, `Provenance`, `StageProvenance`, `FitStatus`, `FitQuality`, `CrossingStatus`, `SelectionMode`, `FallbackReason`, `RobustnessFlags`, `StageTiming`, `MetricBreakdown`, `MetricComponent`, etc.)
- `color.rs` — sRGB ↔ linear RGB (IEC 61966-2-1), CIE Y luminance extraction, lightness-based RGB reconstruction
- `resize.rs` — Lanczos3 downscale via `image` crate
- `sharpen.rs` — unsharp mask (3-channel RGB and single-channel luminance) with hand-rolled separable Gaussian; **deliberately no clamping** so out-of-range values exist for the metric
- `paper_sharpen.rs` — scaffold for paper-style lightness sharpening operator; currently delegates to `sharpen.rs`
- `metrics.rs` — `channel_clipping_ratio` (per-channel fraction outside [0,1]) and `pixel_out_of_gamut_ratio` (per-pixel fraction); selectable via `ArtifactMetric` enum; `compute_metric_breakdown` produces component-wise `MetricBreakdown` (v0.2 composite metric scaffold)
- `fit.rs` — 4×4 Vandermonde normal equations solved by Gaussian elimination with partial pivoting, all in **f64**; `fit_cubic_with_quality` returns `FitQuality` (R², residuals, min pivot); `check_monotonicity` validates probe sample ordering
- `solve.rs` — Cardano's formula for cubic roots + fallback to best probe sample; returns `SolveResult` with `SelectionMode` and `CrossingStatus`
- `contrast.rs` — placeholder contrast leveling (percentile stretch); real formula unknown
- `pipeline.rs` — orchestrates all stages; measures baseline, supports lightness/RGB sharpening and absolute/relative metric modes; dispatches on `SharpenModel` and `ArtifactMetric`; emits per-stage `Provenance` tags; computes `FitQuality`, `RobustnessFlags` (monotonicity + LOO stability), typed `FallbackReason`, per-stage `StageTiming`, and `MetricBreakdown` per probe; public entry point is `process_auto_sharp_downscale`

**`r3sizer-io`** — `load_as_linear` (file → `LinearRgbImage`, applies sRGB→linear) and `save_from_linear` (applies linear→sRGB, writes file). Format inferred from extension.

**`r3sizer-cli`** — thin wrapper: `args.rs` (clap), `run.rs` (load→process→save), `output.rs` (stdout formatting), `sweep.rs` (batch directory processing with aggregate statistics).

**`r3sizer-wasm`** — WebAssembly bindings (`wasm-bindgen`). Single entry point: `process_image(srgb_rgba_data, width, height, params_json) → JsValue`. Accepts sRGB RGBA u8 pixels (canvas `getImageData()`), returns a JS object with `imageData` (Uint8Array), `outputWidth`, `outputHeight`, and `diagnostics`. Depends on `r3sizer-core` with `default-features = false` (no rayon). Color conversion between RGBA u8 and `LinearRgbImage` is in `convert.rs`.

**`web/`** — React 19 + Vite + Tailwind diagnostic UI. Communicates with `r3sizer-wasm` via a Web Worker (`wasm.ts` / `wasm-worker.ts`). State managed by Zustand (`processor-store.ts`). TypeScript types are auto-generated from Rust via `ts-rs` (see below).

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
- **Composite metric scaffold is in place** — `MetricBreakdown` with `MetricComponent` variants (GamutExcursion, HaloRinging, EdgeOvershoot, TextureFlattening) is populated per probe. For v0.1, only GamutExcursion is active; others return 0.0. The `aggregate` field preserves backward compatibility with the scalar fitting path.
- **TypeScript types are generated from Rust, not handwritten** — `ts-rs` with `serde-compat` reads `#[serde(...)]` attributes and produces matching TypeScript. The `typegen` feature flag keeps `ts-rs` out of production builds. Default constants are serialized from Rust `Default` impls so they stay in sync. `generated.ts` must be committed; the Docker build regenerates it to ensure freshness.

## Algorithm summary

The core idea: select sharpening strength by constraining the fraction of channel values that fall outside [0,1] after sharpening (artifact ratio P) to a target threshold P0 (default 0.1%). A cubic P(s) is fitted to probe measurements, then solved for s*.

```
linearize → downscale → [contrast leveling] → measure baseline P(base) →
extract luminance L → probe N strengths { sharpen L(s_i) → reconstruct RGB → measure P(s_i) } →
compute metric_value per mode → fit cubic P_hat(s) (with quality metrics) →
robustness checks (monotonicity, LOO) → solve P_hat(s*) = P0 →
final sharpen(s*) → clamp → output
```

See `docs/algorithm.md`, `docs/assumptions.md`, and `docs/future_work.md` for details on confirmed vs. approximated paper details and the Tauri integration path.
