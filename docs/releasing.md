# Releasing r3sizer crates

How to version, verify, and publish the workspace crates to crates.io.
Releases are manual; there is no release workflow. Publishing is permanent, so
follow the order below.

## What gets published

| Crate | Published | Internal dependencies |
| --- | --- | --- |
| `r3sizer-metadata` | yes — **not yet on crates.io**; the first publish claims the name | none |
| `r3sizer-core` | yes (0.9.0 on crates.io) | dev-only: `r3sizer-metadata` (typegen) |
| `r3sizer-io` | yes (0.9.0) | `r3sizer-core`, `r3sizer-metadata` |
| `r3sizer` (CLI) | yes (0.9.0) | `r3sizer-core`, `r3sizer-io` |
| `r3sizer-wasm` | no (`publish = false`) | `r3sizer-core`, `r3sizer-metadata` |

The web app (`web/package.json`, `"private": true`) is not published; it ships
through GitHub Pages on every push to `main` (`.github/workflows/deploy.yml`).
Its `version` field is independent of the crate version.

## Versioning

All crates share one version: `[workspace.package] version` in the root
`Cargo.toml`. They are always bumped and released together.

The project is pre-1.0. Cargo treats `0.x` minor bumps as breaking, so:

- **Minor** (`0.9.0` → `0.10.0`): new features, public API additions or
  changes, or user-visible behaviour changes (for example, metadata being
  preserved by default).
- **Patch** (`0.9.0` → `0.9.1`): bug fixes only, with no API or behaviour
  change.

### Where the version lives

1. Root `Cargo.toml` → `[workspace.package] version`.
2. Internal dependency pins, which must match the new minor (`"0.9"` →
   `"0.10"`):
   - `crates/r3sizer-core/Cargo.toml` — dev-dependency `r3sizer-metadata`
   - `crates/r3sizer-io/Cargo.toml` — `r3sizer-core`, `r3sizer-metadata`
   - `crates/r3sizer/Cargo.toml` — `r3sizer-core`, `r3sizer-io`
   - `crates/r3sizer-wasm/Cargo.toml` — `r3sizer-core`, `r3sizer-metadata`

   Find them all with:

   ```sh
   grep -rn 'r3sizer-[a-z]* *= *{ *path' crates/*/Cargo.toml
   ```

3. `Cargo.lock` — refreshed by `cargo check --workspace`.

A patch release does not change the pins (`"0.9"` already accepts `0.9.1`).

## Release procedure

### 1. Start from an up-to-date `main`

Feature work lands on `main` through PRs first. Then:

```sh
git switch main && git pull
git switch -c release/v0.10.0
```

### 2. Bump the version

Edit the root version and the internal pins listed above, then:

```sh
cargo check --workspace   # updates Cargo.lock
```

### 3. Update `CHANGELOG.md`

- Rename `## [Unreleased]` to `## [0.10.0] - YYYY-MM-DD`.
- Add a new, empty `## [Unreleased]` above it.
- Fix the compare links at the bottom of the file:

  ```markdown
  [Unreleased]: https://github.com/alvytsk/r3sizer/compare/v0.10.0...HEAD
  [0.10.0]: https://github.com/alvytsk/r3sizer/compare/v0.9.0...v0.10.0
  ```

- Review each crate's `description`, `keywords`, and `README.md`. crates.io
  shows them, and they should describe the current behaviour.

### 4. Run the CI checks locally

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo test -p r3sizer-core --features typegen export_typescript_bindings -- --nocapture
git diff --exit-code -- web/src/shared/lib/types/generated.ts
wasm-pack test --node crates/r3sizer-wasm
```

Check the declared minimum Rust version (`rust-version = "1.88"`):

```sh
rustup toolchain install 1.88   # once
cargo +1.88 check --workspace
```

If this fails, either fix the code or raise `rust-version` and record that in
the changelog.

### 5. Dry-run the packages

```sh
cargo publish --dry-run -p r3sizer-metadata
cargo package --list -p r3sizer-metadata   # inspect contents; crates.io limit is 10 MB
```

Only crates whose internal dependencies are already on crates.io at the new
version can be dry-run individually. Before anything is published, a dry-run of
`r3sizer-core`, `r3sizer-io`, or `r3sizer` fails with
`no matching package named ... found`. That is expected, not a packaging bug.

With cargo 1.90 or newer you can package all members in dependency order
instead:

```sh
cargo publish --workspace --dry-run
```

Confirm the output lists every published crate before relying on it.

### 6. Merge the release PR

Open a PR for `release/v0.10.0` (for example,
`chore(release): bump workspace to 0.10.0`), wait for CI to pass, and merge it.

### 7. Tag the merged commit

```sh
git switch main && git pull
git tag -a v0.10.0 -m "v0.10.0"
git push origin v0.10.0
```

### 8. Publish, in dependency order

Publish from the tagged commit with a clean working tree:

```sh
cargo login                         # once; see "crates.io token" below
cargo publish -p r3sizer-metadata   # must be first: core, io, and wasm depend on it
cargo publish -p r3sizer-core
cargo publish -p r3sizer-io
cargo publish -p r3sizer
```

Cargo waits for each crate to appear in the index before returning, so the
next command can resolve it. Alternatively, run `cargo publish --workspace`
(cargo 1.90+), which publishes the same crates in the same order and skips
`r3sizer-wasm`.

### 9. Announce

Create a GitHub release from the tag, using the changelog section as notes:

```sh
gh release create v0.10.0 --title "v0.10.0" --notes "<paste the 0.10.0 changelog section>"
```

## crates.io token

- Create a token at <https://crates.io/settings/tokens>.
- Scopes: `publish-update` for existing crates, plus **`publish-new`** for a
  release that introduces a new crate (such as the first `r3sizer-metadata`
  publish).
- Restrict the token to the `r3sizer*` crate pattern, and set an expiry.
- Store it with `cargo login`. Never commit it or paste it into CI logs.

## Rules and gotchas

- **Publishing is permanent.** A version can never be overwritten or deleted.
  `cargo yank --version X -p <crate>` only stops new projects from resolving
  it. Fix mistakes with a new patch release.
- **Order matters.** A dependency must be on crates.io before anything that
  depends on it, including dev-dependencies that carry a `version` (this is
  why `r3sizer-metadata` goes first).
- **Partial failure.** If a publish fails midway, don't republish crates that
  already went out. Fix the problem and continue with the remaining crates in
  order. If a published crate needs a code change, cut a patch release.
- **Never publish from a feature branch or a dirty tree.** `cargo publish`
  refuses a dirty tree unless you pass `--allow-dirty`; don't.
- **`r3sizer-wasm` is never published.** The web app builds it from source.
- **crates.io metadata limits:** at most 5 keywords, categories from
  <https://crates.io/category_slugs>, and a package of 10 MB or less.
