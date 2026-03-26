# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```sh
# Build
cargo build --workspace

# Test all
cargo test --workspace

# Test a single test by name
cargo test -p imgsharp-core <test_name>

# Run integration tests only
cargo test -p imgsharp-core --test integration

# Lint (warnings are errors)
cargo clippy --workspace -- -D warnings

# Run benchmarks
cargo bench -p imgsharp-core

# Run the CLI
cargo run -p imgsharp-cli -- --input <FILE> --output <FILE> --width <N> --height <N>
cargo run -p imgsharp-cli -- --input photo.jpg --output out.png --width 800 --height 600 --diagnostics diag.json
```

## Architecture

Three crates with a strict dependency direction: `imgsharp-core` ← `imgsharp-io` ← `imgsharp-cli`.

**`imgsharp-core`** — all image processing logic, no I/O. This is the library meant to be reused in a future Tauri GUI or WASM build. Modules map 1:1 to pipeline stages:

- `types.rs` — all shared data types (`LinearRgbImage`, `AutoSharpParams`, `ProcessOutput`, `AutoSharpDiagnostics`, `CubicPolynomial`, `ProbeSample`, `SharpenMode`, `MetricMode`, `FitStatus`, `CrossingStatus`, `SelectionMode`, etc.)
- `color.rs` — sRGB ↔ linear RGB (IEC 61966-2-1), CIE Y luminance extraction, lightness-based RGB reconstruction
- `resize.rs` — Lanczos3 downscale via `image` crate
- `sharpen.rs` — unsharp mask (3-channel RGB and single-channel luminance) with hand-rolled separable Gaussian; **deliberately no clamping** so out-of-range values exist for the metric
- `metrics.rs` — `artifact_ratio`: fraction of f32 channel values outside [0,1]
- `fit.rs` — 4×4 Vandermonde normal equations solved by Gaussian elimination with partial pivoting, all in **f64**; takes generic `(x, y)` pairs
- `solve.rs` — Cardano's formula for cubic roots + fallback to best probe sample; returns `SolveResult` with `SelectionMode` and `CrossingStatus`
- `contrast.rs` — placeholder contrast leveling (percentile stretch); real formula unknown
- `pipeline.rs` — orchestrates all stages; measures baseline, supports lightness/RGB sharpening and absolute/relative metric modes; public entry point is `process_auto_sharp_downscale`

**`imgsharp-io`** — `load_as_linear` (file → `LinearRgbImage`, applies sRGB→linear) and `save_from_linear` (applies linear→sRGB, writes file). Format inferred from extension.

**`imgsharp-cli`** — thin wrapper: `args.rs` (clap), `run.rs` (load→process→save), `output.rs` (stdout formatting).

## Key design decisions to preserve

- **f32 for pixels, f64 for polynomial fitting** — the Vandermonde matrix has terms up to `s^6`; f32 causes catastrophic cancellation.
- **No clamping inside `sharpen.rs`** — out-of-range values are the artifact signal. Clamping happens only in `pipeline.rs` at the final output stage.
- **`downscaled` image is never mutated during probing** — each probe in the loop produces a fresh allocation, leaving `base` unchanged for the final apply. In lightness mode, luminance is extracted once and reused across all probes.
- **Fallback is not an error** — when the cubic solve finds no root in the probe range, `solve.rs` falls back to the best probe sample. The pipeline always returns a result. Selection outcome is reported via `SelectionMode` enum (`PolynomialRoot`, `BestSampleWithinBudget`, `LeastBadSample`, `BudgetUnreachable`).
- **Lightness-based sharpening is the default** — `SharpenMode::Lightness` sharpens CIE Y luminance, then reconstructs RGB via `k = L'/L`. This is an engineering approximation (strong inference from paper, not confirmed exact formula). `SharpenMode::Rgb` is kept for comparison.
- **Baseline measurement separates resize from sharpen artifacts** — `MetricMode::RelativeToBase` (default) subtracts the pre-sharpen baseline from each probe measurement, so the fitted metric only reflects sharpening-induced artifacts.
- **`contrast.rs` is a documented stub** — `ContrastLevelingParams::enabled = false` by default. The function signature and placement are fixed; only the body needs replacement once the paper formula is known.

## Algorithm summary

The core idea: select sharpening strength by constraining the fraction of channel values that fall outside [0,1] after sharpening (artifact ratio P) to a target threshold P0 (default 0.1%). A cubic P(s) is fitted to probe measurements, then solved for s*.

```
linearize → downscale → [contrast leveling] → measure baseline P(base) →
extract luminance L → probe N strengths { sharpen L(s_i) → reconstruct RGB → measure P(s_i) } →
compute metric_value per mode → fit cubic P_hat(s) → solve P_hat(s*) = P0 →
final sharpen(s*) → clamp → output
```

See `docs/algorithm.md`, `docs/assumptions.md`, and `docs/future_work.md` for details on confirmed vs. approximated paper details and the Tauri integration path.
