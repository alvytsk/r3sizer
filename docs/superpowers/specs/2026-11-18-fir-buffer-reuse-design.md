# Buffer reuse in `fir_resize` — design

Date: 2026-11-18
Scope: small change (one module + one bench fix)

## Intent and why

`profile_e2e` shows 1080p→540p resize at ~8–10 ms while the identical
convolution measured with reused buffers takes ~1.5 ms. Controlled A/B
(/tmp/opencode probes, 2026-11-18) established the causal variable: per-call
fresh allocations in `fir_resize` (`crates/r3sizer-core/src/resize.rs:88`),
not the resize algorithm, not `fast_image_resize` itself, not rayon
(excluded via `RAYON_NUM_THREADS=1` and a no-rayon build).

Every call today allocates and frees three large buffers
(1080p→540p ≈ 25 MB total):

1. `fir::Resizer::new()` — internal convolution buffer ≈ 12.4 MB (dominant
   churn source; glibc mmap-threshold gray zone → erratic cost),
2. destination `TypedImage` ≈ 6.2 MB,
3. output copy `f32x3_slice_to_vec` ≈ 6.2 MB.

## Goals

- Resize stage ≈ 1.5–2 ms (measured in probes) instead of ~8–10 ms for the
  1080p→540p bench scenario; stable across process contexts.
- No public API changes.
- Bit-identical outputs (no math changes — coefficient computation is
  per-resize and unaffected by reuse).

## Non-goals / out of scope

- WASM gains: plausible (fewer large allocations in linear memory) but
  unmeasured here — needs a separate WASM/browser measurement.
- Probing-stage allocations (same churn pattern, likely inflates `probing`
  timings) — future work, may shift the stage profile.
- Kernel-side mechanism pinpointing (needs `perf`, not installed).

## Approaches considered

1. **`thread_local!` cache in `resize.rs`** (chosen) — zero API change,
   transparent to CLI batch, `prepare_base`, WASM workers (per-worker
   instance). Idiomatic for per-thread scratch state.
2. Plumb a `Resizer` through the API / `PreparedBase` — rejected: invasive
   public-surface change for internal bookkeeping.
3. Caller-owned reuse object (builder style) — rejected: changes every call
   site including wasm bindings.

## The change

In `resize.rs`:

- `thread_local! { static RESIZE_CACHE: RefCell<ResizeCache> }` holding
  - `resizer: fir::Resizer` — persists its internal convolution buffer;
  - `dst: Option<(u32, u32, fir::images::TypedImage<'static, F32x3>)>` —
    reused when dims match, reallocated on mismatch (varying-size batches
    degrade to current behaviour, no regression).
- `fir_resize` uses the cached resizer and dst; copy-out stays `to_vec`
  (the output `Vec` is moved into `LinearRgbImage` — one fresh output
  allocation per call is inherent to ownership, down from three).
- Steady state per call: 1 large fresh allocation (~6.2 MB) instead of
  three (~25 MB); no Resizer re-init.
- Each thread that ever resized retains its buffers for the thread's
  lifetime (max size seen). Accepted; noted in doc comments.

## Bench harness fix (same work item)

`benches/profile_e2e.rs:86–91` selects the run by median `total_us` and
prints that run's per-stage numbers. Change to: per-stage median across the
three iterations (TOTAL stays the median-total run). Prevents single-run
noise from masquerading as stage behaviour.

## Validation

1. `cargo test -p r3sizer-core` (incl. existing resize/staged-shrink tests).
2. New unit test: two consecutive `downscale` calls produce identical
   output (reuse must not change math).
3. `cargo bench --bench profile_e2e`: 1080p→540p resize expected ~2 ms
   (was ~8–10), 4K→720p unchanged (~4.3–6.5); gap between wall and sum of
   stages stays ~1%.
4. `cargo bench --bench pipeline_bench` — no regression in totals.
