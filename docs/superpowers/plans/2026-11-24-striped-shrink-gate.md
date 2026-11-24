# Striped-Shrink Gate Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make striped ingest refuse shrink ratios where its pre-reduce is an identity pass (3.0× up to 4.0×), so the web client sends those jobs down the monolithic path and the docs state the real threshold.

**Architecture:** `compute_intermediate_size` floors the pre-reduce factor to 1 below a 4× shrink, so the "intermediate" is the source itself. The staged resize path already skips staging in that case (`resize.rs:57`, from #36); the striped gate and the web client mirror were left at 3.0. The fix mirrors the existing guard in `validate_striped_shrink`, repeats it in `StripedPreReducer::new` (the layer that allocates), moves the web mirror to 4.0, and corrects the docs that still say 3×. No new public API, no change to the intermediate-size math.

**Tech Stack:** Rust (`r3sizer-core`, `r3sizer-wasm`), `cargo test`, ts-rs typegen, TypeScript + Vitest (`web/`).

**Spec:** https://github.com/alvytsk/r3sizer/issues/45

## Global Constraints

- `compute_intermediate_size` math is unchanged — a striped ingest must land on the same intermediate dimensions as the monolithic path.
- `STAGED_SHRINK_THRESHOLD` stays `3.0`; both gates pair it with `compute_intermediate_size(src, target) != src`.
- No new public API in `r3sizer-core`.
- `generated.ts` is never hand-edited: regenerate with `cargo test -p r3sizer-core --features typegen export_typescript_bindings -- --nocapture` and commit it (CI fails on a diff).
- `web/src/shared/api/processing/wasm-pkg/` is gitignored. Never commit it; the `ingest_begin` doc change reaches its `.d.ts` on the next build.
- The dated records under `docs/superpowers/specs/` and `docs/superpowers/plans/` are history: do not edit them.
- Green at the end of every task: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`; in `web/`: `npm run lint`, `npx tsc -b`, `npm test`.
- Work on branch `fix/striped-shrink-gate` (off `origin/main`). Commit after each task.

## What This Fix Does Not Do

It does not lower peak memory for 3–4× jobs. Static accounting from the code (not measured in a browser): the process-wide peak rises by about 8 B/px, all of it in JS heaps rather than in wasm. The wasm heap peaks at 16 B/px on both routes — monolithic holds a transient RGBA copy (`passArray8ToWasm0`, 4) plus the linear image (12); the identity ingest held `acc` (12) plus `weight` (4). Monolithic additionally holds the RGBA buffer on the main thread and a structured-clone copy in the worker (4 B/px each), where the ingest held only stripes. What changes: the gate and its docs become truthful, the pointless accumulate pass and `weight` buffer go away, and 3–4× large images behave like today's 1–3× range (full-size `OffscreenCanvas` in `bitmapToRgba`, which is not chunk-width-limited like `extractStripes`).

## Review Focus

1. Exactly 4.0× — must be accepted by core and routed to striped by the web client. Pinned by `validate_accepts_ratio_exactly_4x` (Task 1) and "runs the striped ingest at exactly 4x" (Task 2).
2. Just below 4.0× (3.99×) — rejected by core, monolithic in the web client. Pinned by `validate_rejects_ratio_just_below_4x` (Task 1) and the `3.99x` table row (Task 2).
3. Exactly 3.0×, the old boundary — rejected, as the staged path already does. Pinned by `validate_rejects_ratio_exactly_3x` (Task 1) and the `3x` table row (Task 2).
4. A target that shrinks only one axis by ≥ 4× (4000×4000 → 4000×1000) — still accepted; the gate must compare whole sizes, not per axis. Pinned by `validate_accepts_target_that_shrinks_only_one_axis` and `identity_axis_still_reduces` (Task 1).
5. `StripedPreReducer::new(src, src)` called directly, bypassing `validate_striped_shrink` — must not allocate. Pinned by `new_rejects_intermediate_equal_to_source` (Task 1).

---

### Task 1: Core gate — refuse an identity pre-reduce

**Files:**
- Modify: `crates/r3sizer-core/src/ingest.rs:30-44` (`validate_striped_shrink`), `:99-109` (`StripedPreReducer::new`)
- Modify: `crates/r3sizer-core/src/lib.rs:80` (error text)
- Test: `crates/r3sizer-core/src/ingest.rs` (`mod tests`)

**Interfaces:**
- Consumes: `compute_intermediate_size(src: ImageSize, target: ImageSize) -> ImageSize` (existing), `crate::resize::STAGED_SHRINK_THRESHOLD` (existing).
- Produces: signatures unchanged. `validate_striped_shrink` returns `Ok(())` iff `max_shrink >= STAGED_SHRINK_THRESHOLD && compute_intermediate_size(src, target) != src`, else `CoreError::TargetTooCloseForStripedIngest { ratio: max_shrink }`. `StripedPreReducer::new` returns `CoreError::InvalidParams` when `intermediate == src`.

- [ ] **Step 1: Write the failing tests**

In `mod tests` of `crates/r3sizer-core/src/ingest.rs`, replace `validate_accepts_ratio_at_threshold` (it asserts the buggy 3.0 acceptance) with the three boundary tests, and add the rest:

```rust
    #[test]
    fn validate_rejects_ratio_exactly_3x() {
        // 3.0 clears the bare threshold, but the staged path skips it too (the
        // pre-reduce would be an identity pass), so the striped gate must agree.
        let err = validate_striped_shrink(
            ImageSize {
                width: 6000,
                height: 4000,
            },
            ImageSize {
                width: 2000,
                height: 4000,
            },
        )
        .unwrap_err();
        assert!(matches!(
            err,
            CoreError::TargetTooCloseForStripedIngest { .. }
        ));
    }

    #[test]
    fn validate_rejects_ratio_between_3x_and_4x() {
        // 6000x4000 -> 1714x1143 is a 3.5x shrink. It clears the 3.0 threshold,
        // but the pre-reduce factor floors to 1, so the "intermediate" is the
        // source and the ingest would be a full-size identity accumulate.
        let err = validate_striped_shrink(
            ImageSize {
                width: 6000,
                height: 4000,
            },
            ImageSize {
                width: 1714,
                height: 1143,
            },
        )
        .unwrap_err();
        assert!(matches!(
            err,
            CoreError::TargetTooCloseForStripedIngest { .. }
        ));
    }

    #[test]
    fn validate_rejects_ratio_just_below_4x() {
        // Both axes stay under 4x: 3.997x and 3.996x.
        let err = validate_striped_shrink(
            ImageSize {
                width: 6000,
                height: 4000,
            },
            ImageSize {
                width: 1501,
                height: 1001,
            },
        )
        .unwrap_err();
        assert!(matches!(
            err,
            CoreError::TargetTooCloseForStripedIngest { .. }
        ));
    }

    #[test]
    fn validate_accepts_ratio_exactly_4x() {
        // The first ratio with a real pre-reduce (factor 2): the staged path
        // engages here, so striped ingest must too.
        validate_striped_shrink(
            ImageSize {
                width: 6000,
                height: 4000,
            },
            ImageSize {
                width: 1500,
                height: 1000,
            },
        )
        .unwrap();
    }

    #[test]
    fn validate_accepts_target_that_shrinks_only_one_axis() {
        // 4000x4000 -> 4000x1000 leaves x at 1:1, but the intermediate is
        // 4000x2000 — half the rows — so the ingest still bounds memory.
        validate_striped_shrink(
            ImageSize {
                width: 4000,
                height: 4000,
            },
            ImageSize {
                width: 4000,
                height: 1000,
            },
        )
        .unwrap();
    }

    #[test]
    fn new_rejects_intermediate_equal_to_source() {
        // Callers that skip `validate_striped_shrink` must not be able to
        // allocate a full-size identity accumulator.
        let err = StripedPreReducer::new(
            ImageSize {
                width: 64,
                height: 64,
            },
            ImageSize {
                width: 64,
                height: 64,
            },
        )
        .unwrap_err();
        assert!(matches!(err, CoreError::InvalidParams(_)));
    }

    #[test]
    fn identity_axis_still_reduces() {
        // 4x4 -> 4x2: x maps 1:1, y halves. Only a fully identity intermediate
        // is useless; one unreduced axis is legitimate.
        let src = ImageSize {
            width: 4,
            height: 4,
        };
        let rgba = rgba_from_fn(4, 4, |x, y| ((x * 60) as u8, (y * 60) as u8, 0));
        let out = reduce_whole(
            src,
            ImageSize {
                width: 4,
                height: 2,
            },
            &rgba,
        );
        assert_eq!((out.width(), out.height()), (4, 2));
        for y in 0..2usize {
            for x in 0..4usize {
                let i = (y * 4 + x) * 3;
                let rows = [
                    SRGB_U8_TO_LINEAR[(2 * y) * 60],
                    SRGB_U8_TO_LINEAR[(2 * y + 1) * 60],
                ];
                assert!((out.pixels()[i] - SRGB_U8_TO_LINEAR[x * 60]).abs() < 1e-6);
                assert!((out.pixels()[i + 1] - (rows[0] + rows[1]) / 2.0).abs() < 1e-6);
            }
        }
    }
```

This characterization test documents the cliff the gate relies on. It is expected to **pass before the fix** (existing math):

```rust
    #[test]
    fn intermediate_size_steps_at_exactly_4x() {
        // The pre-reduce factor is floor(max_shrink / 2): it first reaches 2
        // at a 4x shrink. This is the cliff both staged gates key on.
        // 4000x3000 -> 1001x752 is 3.996x on both axes at most.
        assert_eq!(
            compute_intermediate_size(
                ImageSize {
                    width: 4000,
                    height: 3000
                },
                ImageSize {
                    width: 1001,
                    height: 752
                },
            ),
            ImageSize {
                width: 4000,
                height: 3000
            }
        );
        assert_eq!(
            compute_intermediate_size(
                ImageSize {
                    width: 4000,
                    height: 3000
                },
                ImageSize {
                    width: 1000,
                    height: 750
                },
            ),
            ImageSize {
                width: 2000,
                height: 1500
            }
        );
    }
```

- [ ] **Step 2: Run the tests to verify the right ones fail**

Run: `cargo test -p r3sizer-core --lib ingest::tests`
Expected: FAIL for `validate_rejects_ratio_exactly_3x`, `validate_rejects_ratio_between_3x_and_4x`, `validate_rejects_ratio_just_below_4x`, `new_rejects_intermediate_equal_to_source` (each on `unwrap_err()` of an `Ok`). PASS for `validate_accepts_ratio_exactly_4x`, `validate_accepts_target_that_shrinks_only_one_axis`, `identity_axis_still_reduces`, `intermediate_size_steps_at_exactly_4x`.

- [ ] **Step 3: Add the guard to `validate_striped_shrink`**

Keep the signature. Replace the single threshold comparison with the staged path's condition:

```rust
    // Same guard as the staged path in `resize::downscale_with_info`.
    if max_shrink < crate::resize::STAGED_SHRINK_THRESHOLD
        || compute_intermediate_size(src, target) == src
    {
        return Err(CoreError::TargetTooCloseForStripedIngest { ratio: max_shrink });
    }
```

Replace the function's doc comment with:

```rust
/// Check that an image qualifies for striped ingest: the staged pre-reduce
/// must be a real reduction, i.e. [`compute_intermediate_size`] must return
/// something smaller than the source. That needs a shrink ratio of at least
/// 4.0 — the same condition as the staged path in
/// [`crate::resize::downscale_with_info`]. From 3.0 up to 4.0 the pre-reduce
/// factor floors to 1, the intermediate is the source itself, and the ingest
/// would degrade into a full-size identity accumulate; the caller should use
/// the monolithic path instead.
```

- [ ] **Step 4: Repeat the guard in `StripedPreReducer::new`**

After the existing "exceeds source" check:

```rust
        if intermediate == src {
            return Err(CoreError::InvalidParams(format!(
                "intermediate {}x{} equals the source: the pre-reduce would not reduce the image",
                intermediate.width, intermediate.height
            )));
        }
```

- [ ] **Step 5: Fix the error text in `crates/r3sizer-core/src/lib.rs:80`**

The current text ends in `(shrink ratio {ratio:.2} < 3.0)`, which is false for a rejected 3.5× target. Use:

```rust
    #[error("target too close to source for large-image mode (shrink ratio {ratio:.2}; the pre-reduce needs 4.0x or more)")]
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p r3sizer-core --lib ingest::tests`
Expected: PASS, all tests in the module.

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green, 0 failed.

- [ ] **Step 7: Commit**

```bash
git add crates/r3sizer-core/src/ingest.rs crates/r3sizer-core/src/lib.rs
git commit -m "fix(core): reject striped ingest when the pre-reduce is an identity pass"
```

---

### Task 2: Web client — mirror the gate at 4.0

**Files:**
- Modify: `web/src/shared/api/processing/client.ts:26-32`
- Test: `web/src/shared/api/processing/client.test.ts`

**Interfaces:**
- Consumes: core semantics from Task 1 (`ingest_begin` now rejects ratios below 4×).
- Produces: `STRIPED_INGEST_MIN_SHRINK: number` (existing export), value `4.0`.

- [ ] **Step 1: Write the failing tests**

In `web/src/shared/api/processing/client.test.ts`, replace `"falls back to the monolithic path when a large image has a modest downscale"` with a table that includes the 3–4× window, and add the boundary test:

```ts
  it.each([
    ["2x", 4000, 3000],
    ["3x", 2667, 2000],
    ["3.5x", 2286, 1714],
    ["3.99x", 2005, 1504],
  ] as const)(
    "falls back to the monolithic path at a %s shrink of a large image",
    async (_ratio, targetWidth, targetHeight) => {
      // 8000x6000 (48MP) is classified striped at decode, but below a 4x
      // shrink the pre-reduce factor floors to 1: the striped ingest would be
      // an identity pass over the whole source and core rejects it. The job
      // must run monolithic instead.
      await client.decode(new File([], "big.jpg"));
      await client
        .process({ ...params(), target_width: targetWidth, target_height: targetHeight })
        .promise;
      expect(wasm.ingestBegin).not.toHaveBeenCalled();
      expect(wasm.processImageParallel).toHaveBeenCalledWith(
        expect.objectContaining({ length: 8000 * 6000 * 4 }),
        8000,
        6000,
        expect.any(String),
        expect.anything(),
      );
    },
  );

  it("runs the striped ingest at exactly 4x", async () => {
    // The first ratio with a real pre-reduce: the striped path must still take it.
    await client.decode(new File([], "big.jpg"));
    await client
      .process({ ...params(), target_width: 2000, target_height: 1500 })
      .promise;
    expect(wasm.ingestBegin).toHaveBeenCalledWith(8000, 6000, 2000, 1500);
  });
```

`"runs the striped ingest at exactly 4x"` is a characterization test and is expected to **pass before the fix**; it guards against over-correcting to `>`.

- [ ] **Step 2: Run the tests to verify the right ones fail**

Run (from `web/`): `npx vitest run src/shared/api/processing/client.test.ts`
Expected: FAIL for table rows `3x`, `3.5x`, `3.99x` (`ingestBegin` was called). PASS for row `2x` and for `"runs the striped ingest at exactly 4x"`.

- [ ] **Step 3: Move the mirror to 4.0 and correct its doc**

In `web/src/shared/api/processing/client.ts`:

```ts
/**
 * Minimum shrink ratio (max of the two axes) the striped ingest path accepts.
 * Below 4.0 the pre-reduce factor floors to 1, so the "intermediate" would be
 * the source itself and the ingest a full-size identity pass; core rejects it
 * (`validate_striped_shrink`, which mirrors the staged-shrink guard in
 * `resize.rs`). A large image with a modest downscale falls back to the
 * monolithic path.
 */
export const STRIPED_INGEST_MIN_SHRINK = 4.0;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run (from `web/`): `npx biome check --write src/shared/api/processing/client.test.ts src/shared/api/processing/client.ts`, then `npm run lint && npx tsc -b && npm test`
Expected: lint and type-check clean; all web tests pass.

- [ ] **Step 5: Commit**

```bash
git add web/src/shared/api/processing/client.ts web/src/shared/api/processing/client.test.ts
git commit -m "fix(web): route large images below a 4x shrink down the monolithic path"
```

---

### Task 3: Pin the 4× cliff on both paths

Test-only. Nothing here changes production code.

**Files:**
- Test: `crates/r3sizer-core/src/resize.rs` (`mod tests`, staged shrink section)
- Test: `crates/r3sizer-core/tests/striped_ingest.rs`

**Interfaces:**
- Consumes: `downscale_with_info(&LinearRgbImage, ImageSize) -> Result<(LinearRgbImage, bool), CoreError>` (existing), `validate_striped_shrink` (Task 1).
- Produces: nothing.

- [ ] **Step 1: Add the monolithic-side boundary test**

In the staged shrink section of `crates/r3sizer-core/src/resize.rs` tests, next to `ratio_below_4x_skips_identity_pre_reduce`:

```rust
    #[test]
    fn ratio_exactly_4x_uses_staged_shrink() {
        // The other side of the cliff: at 4x the pre-reduce factor first
        // reaches 2, so staging engages.
        let src = gradient_image(400, 300);
        let (out, staged) = downscale_with_info(
            &src,
            ImageSize {
                width: 100,
                height: 75,
            },
        )
        .unwrap();
        assert_eq!(out.width(), 100);
        assert_eq!(out.height(), 75);
        assert!(staged, "4× ratio should trigger staged shrink");
    }
```

Characterization: expected to **pass before and after**.

- [ ] **Step 2: Add a parity test just above the gate**

In `crates/r3sizer-core/tests/striped_ingest.rs`, add `validate_striped_shrink` to the `r3sizer_core` import, move the body of `striped_ingest_selects_similar_strength_to_monolithic` into a helper, and call it for two targets:

```rust
/// Gate, reducer and pipeline together, in the order `ingest_begin` uses them.
fn assert_striped_matches_monolithic(target: ImageSize) {
    let (w, h) = (2400u32, 1600u32);
    let rgba = synthetic_rgba(w, h);
    let src = ImageSize {
        width: w,
        height: h,
    };
    validate_striped_shrink(src, target).unwrap();
    // ... existing body from `let params = AutoSharpParams {` to the end ...
}

#[test]
fn striped_ingest_selects_similar_strength_to_monolithic() {
    assert_striped_matches_monolithic(ImageSize {
        width: 400,
        height: 267,
    }); // ratio 6
}

#[test]
fn striped_ingest_just_above_the_4x_gate_matches_monolithic() {
    // 2400x1600 -> 533x355 is ~4.5x: the window the old 3.0 gate let through
    // and no test covered, just past the 4.0x boundary.
    assert_striped_matches_monolithic(ImageSize {
        width: 533,
        height: 355,
    });
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p r3sizer-core --lib resize::tests && cargo test -p r3sizer-core --test striped_ingest`
Expected: PASS, including the new ~4.5× parity case within the existing `0.3` strength tolerance. If the 4.5× case exceeds the tolerance, stop and investigate the intermediate sizes before touching the tolerance.

- [ ] **Step 4: Commit**

```bash
git add crates/r3sizer-core/src/resize.rs crates/r3sizer-core/tests/striped_ingest.rs
git commit -m "test(core): pin the 4x staged-shrink cliff on both ingest paths"
```

---

### Task 4: Correct the docs that still say 3×, and note the change

**Files:**
- Modify: `crates/r3sizer-core/src/resize.rs:17-18, 22-25`
- Modify: `crates/r3sizer-core/src/types.rs:1398-1399` (+ regenerated `web/src/shared/lib/types/generated.ts`)
- Modify: `crates/r3sizer-wasm/src/lib.rs:111-113`
- Modify: `docs/architecture.md:175`, `docs/future_work.md:53`, `CHANGELOG.md` (`[Unreleased]` → `Fixed`)

**Interfaces:**
- Consumes: nothing.
- Produces: nothing (docs and one generated file).

Prose earns no test. Verification is the CI toolchain accepting the result.

- [ ] **Step 1: `resize.rs` — state that the threshold is necessary, not sufficient**

Doc comment on `STAGED_SHRINK_THRESHOLD`:

```rust
/// Lower bound of the shrink-ratio window in which the two-stage path can
/// engage. Necessary but not sufficient: the pre-reduce to ~2x the target
/// only does real work when [`crate::ingest::compute_intermediate_size`]
/// returns something smaller than the source. Its factor is
/// `floor(max_shrink / 2)`, so the two-stage path really starts at a 4x
/// shrink; from this threshold up to 4x the factor floors to 1 and staging
/// is skipped.
```

Replace "For shrink ratios above `STAGED_SHRINK_THRESHOLD` a two-stage path is used" in the `downscale_with_info` doc with:

```rust
/// For shrink ratios of 4x and above a two-stage path is used: a fast bilinear
/// pre-reduce brings the image to ~2× the target, then a final Lanczos3 pass
/// produces the output.  This follows the same principle as libvips' `gap`
/// parameter.  Below 4x the pre-reduce would not shrink the image, so those
/// ratios go single-stage.
```

- [ ] **Step 2: `types.rs:1399` and regenerate `generated.ts`**

Change `/// Active for shrink ratios above ~3×.` to `/// Active for shrink ratios of 4× and above.` Then run the typegen command from Global Constraints and confirm `git diff -- web/src/shared/lib/types/generated.ts` shows exactly that one comment line change.

- [ ] **Step 3: `r3sizer-wasm/src/lib.rs:111-113`**

```rust
/// Begin a striped ingest for a large image. Validates that the shrink ratio
/// gives a real pre-reduce (>= 4x), computes the intermediate size, and
/// invalidates all caches from the previous image. Returns `{ width, height }`
/// of the intermediate.
```

- [ ] **Step 4: `docs/architecture.md:175` and `docs/future_work.md:53`**

- `architecture.md`: `alt Over 24 MP and shrink ratio at least 4x`
- `future_work.md`: replace "`validate_striped_shrink` rejects shrink ratios below 3x, where the pre-reduce would be meaningless." with "`validate_striped_shrink` rejects shrink ratios below 4x, where the pre-reduce factor floors to 1 and the pre-reduce would be a full-size identity pass."

- [ ] **Step 5: `CHANGELOG.md` — new bullet at the end of `[Unreleased]` → `Fixed`**

```markdown
- Striped ingest no longer accepts shrink ratios between 3× and 4×. There the
  pre-reduce factor floors to 1, so the "pre-reduce" was an identity pass over
  a full-size accumulator; `validate_striped_shrink` now applies the same guard
  as the staged resize path, and the web client sends those jobs down the
  monolithic path like smaller downscales.
```

- [ ] **Step 6: Verify the full CI set locally**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`
Run (from `web/`): `npm run lint && npm run fsd && npx tsc -b && npm test`
Run: `cargo test -p r3sizer-core --features typegen export_typescript_bindings -- --nocapture && git diff --exit-code -- web/src/shared/lib/types/generated.ts`
Expected: all green. Pre-existing noise only: `package.homepage is redundant with package.repository` manifest warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/r3sizer-core/src/resize.rs crates/r3sizer-core/src/types.rs crates/r3sizer-wasm/src/lib.rs docs/architecture.md docs/future_work.md CHANGELOG.md web/src/shared/lib/types/generated.ts
git commit -m "docs: state the real 4x threshold for staged shrink and striped ingest"
```
