# Contributing to r3sizer

## Development setup

```sh
# Prerequisites: Rust stable toolchain
rustup update stable

# Build everything
cargo build --workspace

# Run all tests
cargo test --workspace

# Lint (warnings are treated as errors in CI)
cargo clippy --workspace --all-targets -- -D warnings

# Format
cargo fmt --all

# Run benchmarks (optional)
cargo bench -p r3sizer-core
```

### WASM build (optional)

```sh
cargo install wasm-pack
wasm-pack build --target web crates/r3sizer-wasm
```

### Web UI (optional)

```sh
cd web
npm ci
npm run build:wasm    # copies WASM output into web/src/wasm/
npm run dev           # starts Vite dev server
```

### TypeScript type regeneration

When you change serializable types in `r3sizer-core/src/types.rs`, regenerate the
TypeScript bindings and commit the result:

```sh
cargo test -p r3sizer-core --features typegen export_typescript_bindings -- --nocapture
# Writes web/src/types/generated.ts
git add web/src/types/generated.ts
```

## Project structure

```
crates/
  r3sizer-core/   Pure image-processing library — no I/O.  This is the heart
                  of the project; all algorithm work lives here.
  r3sizer-io/     File I/O bridge: load PNG/JPEG/… → LinearRgbImage, save back.
  r3sizer/        CLI (clap subcommands: process, sweep, diff, corpus, presets).
  r3sizer-wasm/   WebAssembly bindings consumed by the web UI.
web/              React 19 + Vite + Tailwind diagnostic UI.
docs/             Algorithm notes, assumptions, CLI reference.
```

Dependency direction is strict: `r3sizer-core` ← `r3sizer-io` ← `r3sizer`,
and `r3sizer-core` ← `r3sizer-wasm`.  `r3sizer-core` must never depend on I/O.

## Pull request checklist

Before opening a PR, make sure:

- [ ] `cargo fmt --all` produces no diff
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` is clean
- [ ] `cargo test --workspace` passes
- [ ] `cargo doc --workspace --no-deps` is warning-free (`RUSTDOCFLAGS="-D warnings"`)
- [ ] New public API has doc comments; internal helpers don't need them
- [ ] Unsafe code has a `// SAFETY:` comment explaining the invariant
- [ ] If you changed `types.rs`, regenerated `web/src/types/generated.ts`

CI enforces all of the above on every push and PR.

## Stability tiers

Modules in `r3sizer-core` are tagged with a stability tier in their module doc:

- **Stable** — `pipeline`, `types` (core subset), `color`, `chroma_guard`.
  Breaking changes require a semver major bump.
- **Experimental** — `evaluator`, `base_quality`, `contrast`, `recommendations`.
  May change in any minor version while the algorithm is being refined.

Use `r3sizer_core::prelude::*` to import the stable public surface only.

## Commit style

Imperative subject line, ≤ 72 characters.  No ticket numbers required.
Keep the body focused on *why*, not *what* (the diff shows what).

## Reporting bugs

Open a GitHub issue with:
- r3sizer version or commit SHA
- Input image characteristics (dimensions, format, content type)
- The command / code that triggered the bug
- Expected vs. actual behaviour

For security vulnerabilities, see [`SECURITY.md`](SECURITY.md) instead.
