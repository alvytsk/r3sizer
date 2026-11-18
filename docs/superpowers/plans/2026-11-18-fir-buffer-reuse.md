# FIR buffer reuse implementation plan

> **For agentic workers:** Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Reuse FIR scratch and destination buffers without changing pixels or the public API; report independent stage medians in the profiler.

**Architecture:** Keep a thread-local `RefCell<ResizeCache>` in `resize.rs`. Retain one Resizer and one destination image, replacing the destination when dimensions change. Copy the result into the caller-owned image as before.

**Tech Stack:** Rust, fast_image_resize 6.0.0, Criterion.

**Spec:** `docs/superpowers/specs/2026-11-18-fir-buffer-reuse-design.md`

## Global constraints

- No public API or dependency changes; bit-identical output.
- One fresh output allocation remains; WASM speedup is unmeasured.
- Scratch retains its high-water allocation for the thread lifetime; destination retains only its current dimensions.

## Review focus

- Repeated calls must overwrite cached pixels without modifying previously returned images.
- Changing dimensions and filters must match a fresh FIR reference, including staged shrink.
- Concurrent caller threads must not share mutable scratch state.
- Optional timing fields must stay absent when the stage does not run.
- Independent stage medians must not be summed against a different run's wall time to manufacture a phase gap.

### Task 1: Cache FIR buffers

**Files:** `crates/r3sizer-core/src/resize.rs` (implementation and tests).
**Interfaces:** Preserve `fir_resize` and all public downscale signatures. Pipeline, resize strategy and ingest callers need no changes.

- [x] Record baseline core tests, profile_e2e and Criterion pipeline_bench.
- [x] Add output-equivalence tests using an independent fresh FIR reference: changed content, dimensions, filter, staged shrink and caller threads. Run before changing production code; correctness should already pass, while the measured performance baseline is the failing acceptance check.
- [x] Add the thread-local Resizer and optional destination, retaining existing error conversion and copy-out.
- [x] Run core tests with default features and without default features; compare profile_e2e and pipeline_bench to baseline.

### Task 2: Report stage medians

**Files:** `crates/r3sizer-core/benches/profile_e2e.rs`.
**Interfaces:** Bench-local median aggregation over three `StageTiming` samples; no library changes.

- [x] Add a small assert-based profiler self-check using samples where the median-total run is not the median-stage run, including optional fields.
- [x] Compute each displayed stage independently. Keep phase gaps internally consistent by deriving them from the original median-total sample, clearly labeling that breakdown.
- [x] Run the self-check and the profiler; run formatting, workspace tests, Clippy and a WASM compile check if the target is installed.
- [x] Review the final diff independently and record measured results and limitations.

## Execution notes

Implementation is authorized by the supplied handoff. Execute inline in the managed worktree; no additional design approval is needed. Do not merge or publish.

Ruling: correctness comparisons are characterization tests and are expected to pass before this performance-only change; the baseline benchmark supplies evidence of the original performance problem.

Ruling: phase breakdown keeps values from one real run, because a sum of independent medians is not a measurement of that run's unaccounted time.

Ruling: a Rayon caller can run another resize while waiting for nested parallel work. Use `try_borrow_mut` and temporary buffers on reentry instead of panicking. The normal path retains its buffers. A held-borrow regression test failed with `RefCell already borrowed` before the fix and passed afterward; an additional two-worker parallel caller test checks the real scheduling path.

Verification: workspace tests passed (554 tests, 5 ignored); core without default features passed (331 tests, 2 ignored). Formatting, Clippy (`--workspace --all-targets -- -D warnings`) and `cargo check -p r3sizer-wasm --target wasm32-unknown-unknown` passed. Existing Cargo manifest warnings remain. The median self-check failed with 5 instead of 40 before the change and passed afterward.

Independent review: the reviewer identified the Rayon reentry risk above; after the fix, no further findings.

## Measured results (2026-11-18)

`profile_e2e`, microseconds; before uses the original median-total selection, after uses independent stage medians. The final after run uses a clean build in a separate target directory.

| Resize scenario | Before | After |
| --- | ---: | ---: |
| 1080p to 540p Fast | 10005 | 3190 |
| 1080p to 540p Balanced | 9436 | 1400 |
| 1080p to 540p Quality | 10255 | 1262 |
| 4K to 720p Fast | 6618 | 4064 |
| 4K to 720p Balanced | 6925 | 4775 |
| 4K to 360p Fast | 5169 | 3938 |
| 4K to 360p Balanced | 4899 | 3203 |

The full `pipeline_bench` run completed. Warmed full-pipeline estimates did not show the large resize improvement seen in the short profiler. Criterion flagged Balanced at +4.46% in the first before/after comparison; follow-up separately built binaries in before/after/after/before order gave central estimates of 25.623 / 25.956 / 25.732 / 25.728 ms (50 samples, 1 s warmup, 3 s measurement). The 4.46% slowdown did not repeat; the new binary was between essentially unchanged and about 1.3% slower in those follow-up runs. Do not claim a demonstrated whole-pipeline speedup or universally sub-2-ms resize.

Artifacts are in `/tmp/r3sizer-reuse-*.log`; Criterion baseline is in the source checkout's ignored `crates/r3sizer-core/target/criterion`. Separate final builds use `/tmp/r3sizer-fir-before` and `/tmp/r3sizer-fir-after` to avoid Cargo fingerprint reuse across checkouts sharing one target directory.

Remaining limits: output copy still allocates; scratch capacity is retained until thread exit; changing destination dimensions reallocates it, including the two staged passes. WASM was compile-checked only. Work remains in the managed worktree; no merge or publication was performed.
