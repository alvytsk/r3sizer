# Changelog

All notable changes to this project will be documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project is pre-1.0 — breaking changes may occur in any release.

---

## [Unreleased]

### Added

#### CI / toolchain
- New **Rust CI workflow** (`.github/workflows/ci.yml`) running on every push
  and PR: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test
  --workspace`, `cargo doc` with `RUSTDOCFLAGS="-D warnings"`, and a WASM build
  via `wasm-pack`.
- New **weekly security audit** (`.github/workflows/audit.yml`) using
  `rustsec/audit-check`, also triggered on any `Cargo.lock` change.

#### CLI — subcommand migration
- CLI restructured from flag-multiplexed modes to **clap subcommands**:
  `process`, `sweep`, `diff`, `corpus`, and `presets list` / `presets show`.
- `process` gains `--output-format json` for machine-readable stdout output.
- `sweep` flags renamed: `--sweep-dir` → `--in-dir`, `--sweep-output-dir` →
  `--out-dir`, `--sweep-summary` → `--summary`.
- New integration test suite (`crates/r3sizer/tests/cli.rs`) using `assert_cmd`
  + `predicates` covering 10 scenarios.

#### Decode limits (`r3sizer-io`)
- New `DecodeLimits` struct with `max_pixels` (default 100 MP) and
  `max_dimension` (default 16 384 px) fields.
- New `load_as_linear_with_limits(path, &limits)` function that reads the image
  header before allocating the pixel buffer, returning `IoError::TooLarge` on
  oversized inputs.
- `load_as_linear` is now a thin wrapper around `load_as_linear_with_limits`
  with default limits — existing callers get protection automatically.
- New `IoError::TooLarge { width, height }` variant.
- CLI flags `--max-pixels` and `--max-dimension` wired into `process` and
  `sweep` subcommands.

#### API quality (`r3sizer-core`)
- New `r3sizer_core::prelude` module re-exporting the stable public surface:
  `LinearRgbImage`, `AutoSharpParams`, preset constructors, pipeline entrypoints
  (`process_auto_sharp_downscale`, `prepare_base`, `process_from_prepared`,
  `PreparedBase`), output types, and the `CoreError` type.
- `PreparedBase::compute_detail(&self, params)` — convenience wrapper around the
  free function `compute_probe_detail`.
- `PreparedBase::run_probes(&self, strengths, params)` — convenience wrapper
  around `run_probes_standalone`.
- Stability tier doc-comments added to experimental modules: `evaluator`,
  `base_quality`, `contrast`, `recommendations`.

#### Examples (`r3sizer-io/examples/`)
- `single_file.rs` — load → `process_auto_sharp_downscale` → save, ~40 lines.
- `two_phase.rs` — `prepare_base` once, run `process_from_prepared` with
  `Fast`, `Balanced`, and `Quality` `PipelineMode` settings.
- `custom_params.rs` — manual `AutoSharpParams` construction; contrasts
  `Uniform` + `SharpenMode::Rgb` against `ContentAdaptive` + `SharpenMode::Lightness`.

#### Repo hygiene
- `SECURITY.md` — threat model, decode-limit mitigations, vulnerability
  reporting address.
- `CONTRIBUTING.md` — dev setup, project structure, PR checklist, stability
  tiers, commit style.
- `CHANGELOG.md` — this file.

### Changed

- **Crate renamed:** `r3sizer-cli` → `r3sizer` so that `cargo install r3sizer`
  works naturally.  The produced binary name (`r3sizer`) is unchanged.
- `README.md` CLI examples updated to use the new subcommand syntax
  (`r3sizer process -i … -o …`).

---

[Unreleased]: https://github.com/alvytsk/r3sizer/compare/HEAD...HEAD
