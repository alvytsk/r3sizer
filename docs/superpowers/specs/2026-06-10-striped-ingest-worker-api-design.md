# Striped Ingest + Processing Worker API — Design

**Date:** 2026-06-10
**Status:** Approved (brainstorming session)
**Scope:** Large-image support (100MP+) in the browser via striped (tiled) ingest, plus a processing worker API with progress and cancellation. Same core mechanism reusable for the future Tauri GUI.

## 1. Problem

The current web pipeline materializes two full-size buffers for every image:

1. Main thread decodes the file into a full-size RGBA8 `ImageData` (`web/src/lib/image-loader.ts`) — ~400 MB for 100MP.
2. The WASM worker converts it to linear f32 (3 channels × 4 bytes = 12 bytes/px) and caches it — ~1.2 GB for 100MP.

A 100MP panorama therefore exceeds the wasm32 heap long before processing starts. Separately, the worker protocol has progress messages but no cancellation: once a WASM call starts, nothing can interrupt it, and the UI cannot abort a long job.

## 2. Goals

- Process 100MP+ source images in the browser with a **bounded WASM heap** (tens of MB, not GB). Note: this bounds the WASM heap, not total browser memory — `createImageBitmap(file)` still requires browser-managed decoded-image memory and can fail for huge images (especially Safari or constrained environments); that failure surfaces as a decode-stage error.
- Give the whole pipeline stage-level progress and cooperative cancellation.
- Keep the new ingest mechanism in `r3sizer-core` (no I/O) so Tauri and CLI can reuse it.
- Package the web-side machinery behind a clean internal facade (no npm publication yet, but nothing blocking it later).

Non-goals: npm package, WASM-side decoding, tiling the post-downscale pipeline (it operates on a small image and needs no tiling), streaming file decode in `r3sizer-io` (future work).

## 3. Key insight

The core already implements **staged shrink**: for shrink ratios ≥ 3×, a bilinear pre-reduce to ~2× the target size precedes the final Lanczos3 pass. For 100MP inputs with typical targets (800–2000 px) the ratio is always large, so the staged path always applies — and a bilinear/area pre-reduce is trivially streamable. It needs only sequential row stripes of the source, never the whole image. Everything after the pre-reduce (final Lanczos3, classification, baseline, probing, fit, sharpen) runs on the small intermediate image (~2MP) and is unchanged.

Memory for 100MP (e.g., 40000×2500): one RGBA8 stripe (~10 MB) + the intermediate linear image (~23 MB for 1600×1200×3×f32) + output. Total: tens of MB in the WASM heap instead of ~1.6 GB.

## 4. Architecture overview

Two preparation paths, selected by a pixel-count threshold (default **24MP**, configurable constant in the web layer):

- **Monolithic (current path)** — images at or below the threshold. Unchanged: full RGBA8 → WASM → linear f32 → pipeline. The full adaptive feature set (content-adaptive resize, `full_diagnostics`) remains available.
- **Striped (new path)** — images above the threshold. The browser decodes the file into an `ImageBitmap` (pixels live in browser/GPU memory, outside the WASM heap). JS extracts row stripes and feeds them to WASM one at a time. WASM accumulates a **streaming area-weighted pre-reduce** directly into the intermediate image (~2× target). The existing pipeline then runs from the intermediate.

Constraints and degradations on the striped path (all recorded in diagnostics):

- **Shrink ratio must be ≥ 3×.** Otherwise the pre-reduce-to-2×-target is meaningless. A large input with a target close to source size yields a typed error ("target too close to source for large-image mode") — the monolithic path would OOM there anyway.
- **Content-adaptive resize is unavailable** (it classifies the full source and performs one full resize per kernel). Resize is forced to uniform; recorded in diagnostics.
- **`full_diagnostics` source-side metrics are skipped** (they need the full source).
- Diagnostics carry the original source dimensions and a `striped: true` flag.

Numerical note: the final result is **not byte-identical** to the current staged path. The final Lanczos3 pass uses the same implementation and parameters, but the streamed area-average intermediate is numerically close — not bit-identical — to the current bilinear pre-reduce. All comparisons in tests use tolerances.

The pixel-count threshold is a **web-layer product policy**, not a core policy: core provides the building blocks; the app decides when to enter striped mode (`DEFAULT_STRIPED_INGEST_THRESHOLD_PIXELS = 24_000_000`).

## 5. Core API (`r3sizer-core`, new module `ingest.rs`)

```rust
pub struct StripedPreReducer { /* accumulator + sRGB→linear LUT + next_row */ }

impl StripedPreReducer {
    /// `intermediate` must come from `compute_intermediate_size` (same logic
    /// as the staged-shrink path, ~2× the final target).
    pub fn new(src: ImageSize, intermediate: ImageSize) -> Result<Self, CoreError>;

    /// Full-width stripes, strictly sequential top-to-bottom.
    /// Accepts sRGB RGBA8; linearization happens inside via a 256-entry LUT.
    /// Out-of-order or oversized input is a CoreError.
    pub fn push_srgb8_rows(&mut self, rgba: &[u8], rows: u32) -> Result<(), CoreError>;

    /// Errors if not all source rows were supplied.
    pub fn finish(self) -> Result<LinearRgbImage, CoreError>;
}

pub fn compute_intermediate_size(src: ImageSize, target: ImageSize) -> ImageSize;
```

Implementation details:

- Area-weighted accumulation: each source pixel contributes to 1–2 output pixels per axis. Precompute the x/y mapping weights once in `new()` so per-pixel work never recomputes coordinate overlaps.
- Store **RGB accumulators plus a separate weight accumulator**, normalize in `finish()` — this avoids edge errors when dimensions do not divide evenly.
- **Alpha policy (explicit):** input is treated as straight (non-premultiplied) RGBA; alpha is ignored, only RGB participates in resizing. First version only.
- Core knows nothing about `File`, `ImageBitmap`, Canvas, OffscreenCanvas, or Tauri. Pure push model — the same code serves WASM today and Tauri (and a potential row-streaming PNG loader in `r3sizer-io`) later.
- Error handling: zero dimensions, invalid stripe buffer length, out-of-order/non-sequential feeding, oversupplied rows (`push` past `src.height`), incomplete supply at `finish()`.

Diagnostics: new type in `types.rs` (with the `typegen`/`ts-rs` derive; regenerate `web/src/types/generated.ts`), threaded into `AutoSharpDiagnostics`:

```rust
pub struct IngestDiagnostics {
    pub original_width: u32,
    pub original_height: u32,
    pub intermediate_width: u32,
    pub intermediate_height: u32,
    pub striped: bool,
    pub forced_uniform_resize: bool,
    pub skipped_source_diagnostics: bool,
}
```

Pipeline integration: `finish()` produces a `LinearRgbImage` used as the pipeline input. At ratio ~2× the pipeline naturally selects a single Lanczos3 pass — which is exactly the staged path's final pass. `BaseParamsKey` fingerprinting continues to work because the intermediate dimensions participate in the key.

## 6. WASM exports (`r3sizer-wasm`)

Symmetric with the existing thread-local cache protocol:

| Export | Action |
|---|---|
| `ingest_begin(src_w, src_h, target_w, target_h)` | creates the thread-local `StripedPreReducer`; returns intermediate dimensions |
| `ingest_stripe(rgba, rows)` | feeds the next stripe |
| `ingest_end()` | `finish()` → stores the intermediate in the **same thread-local input cache that `prepare_image` fills**, plus the original dimensions |
| `ingest_abort()` | drops partial ingest state (used by cancellation) |

After `ingest_end()`, the entire downstream protocol — `prepare_base`, `process_image`, the probe pool, `process_from_probes` — works unchanged: the intermediate is indistinguishable from a normal input.

## 7. Web side (`web/src/processing/`)

The clean internal boundary. `wasm.ts`, `wasm-worker.ts`, `probe-pool.ts`, and `probe-worker.ts` move inside this module; one facade is exported:

```ts
const job = client.process(file, params);   // File | ImageData
job.onProgress(({ stage, fraction, overall }) => ...);
job.cancel();                                // → promise rejects with CancelledError
const result = await job.promise;            // ProcessResult, same shape as today
```

One active job at a time. `processor-store.ts` switches to the facade and stops knowing about workers.

### Stripe extraction (`ingest.ts`)

- Decode via `createImageBitmap(file)`; pixels stay in browser-managed memory.
- Extract each stripe in chunks ≤ 4096 px wide through one reused `OffscreenCanvas`: `drawImage(bitmap, sx, sy, …)` → `getImageData` → assemble a full-width `Uint8Array`, transferred to the worker as a transferable (zero-copy).
- Stripe height targets ~16 MB per message: `stripeH = clamp(16MB / (srcW × 4), 16, 1024)` rows.
- `premultiplyAlpha: 'none'`, `colorSpace: 'srgb'` — best-effort match with the current loader's color behavior; perfect cross-browser consistency is not claimed.
- Chunks of ≤ 4096×1024 are safely below all browser canvas limits; no dynamic limit detection (YAGNI).

### Progress model

Five stages with rough weights kept as tunable constants:

| Stage | Granularity | Source |
|---|---|---|
| `decode` | stage event only | browser exposes no decode progress |
| `ingest` | stripes sent / total | main thread |
| `prepare` | stage event | existing worker progress messages |
| `probe` | probes completed / total | probe pool (already batch-counted) |
| `finalize` | stage event | worker |

### Cancellation — cooperative, no `worker.terminate()` in the normal path

- `cancel()` flags the job id: the main thread stops sending stripes and sends `ingest_abort`; the probe pool stops dispatching further batches; responses carrying the cancelled id are dropped.
- Worst-case cancel latency = the longest single WASM call. After ingest everything runs on the ~2MP intermediate, so this is hundreds of milliseconds, acceptable for UI.
- `worker.terminate()` lives only inside `client.reset()` — emergency recovery for a hung worker, with full WASM re-initialization and cache loss. Not used for routine cancel.
- `SharedArrayBuffer` is deliberately not used (GitHub Pages cannot set COOP/COEP headers).

### UI effects

`ProcessingOverlay` gains a real progress bar and a Cancel button; diagnostics show a striped-ingest badge with the original source size.

## 8. Errors and limits

- **Ratio < 3 on a large image** → typed core error; the facade maps it to a user-facing message ("reduce the target size or use a smaller file").
- **Decode failure / ImageBitmap refusal** (Safari has its own ceilings for giant JPEGs) → `decode`-stage error with the file name; images below the threshold keep the current `<img>` path as fallback.
- **Out-of-order or incomplete stripes** → `CoreError` from `ingest_stripe`/`ingest_end`; the facade sends `ingest_abort` and rejects.
- **Dead or hung worker** → per-stage timeout in the facade → `client.reset()` (terminate + WASM re-init + cache reset), job rejects with diagnostics.
- **Cancellation** → `CancelledError`; WASM ingest state is dropped; caches from the previous successful image are untouched.

## 9. Testing

**Core (Rust unit/integration):**

- `StripedPreReducer` vs reference: the same input fed whole through the current staged path vs in stripes through ingest → per-channel comparison of the intermediate with tolerance (e.g., max diff ≤ 1e-4 in linear). Multiple stripe sizes (1 row, 17 rows, normal stripes, one single full stripe) — the result must not depend on slicing.
- Geometry edge cases: odd dimensions, dimensions that do not divide evenly, extremely wide and extremely tall images, awkward targets like 1537×911.
- Ratio boundary: exactly around 3× and slightly below 3×.
- Error cases: out-of-order stripe, under-supplied rows at `finish()`, oversupplied rows, invalid stripe buffer length, zero dimensions, ratio < 3.
- Synthetic images (gradients, checkerboard) where the area-average has a known exact answer.
- Full pipeline from the intermediate: selected s* for a "large" image via ingest ≈ s* via the monolithic staged path (tolerance for the bilinear-vs-area numerical difference).

**WASM/web:**

- Vitest on the facade with a mocked worker: ingest message sequencing, progress aggregation, cancellation at every stage (including mid-probe — batches stop dispatching), reset recovery.
- Manual check in the demo: a real or generated 100MP panorama; WASM heap stays under ~150 MB (Performance/Memory tab); cancel responds in < 1 s.
- Existing tests untouched: the monolithic path does not change.

## 10. Implementation priority

1. Core `StripedPreReducer` and tests.
2. WASM ingest exports and cache integration.
3. Web processing facade and worker protocol.
4. Stripe extraction from ImageBitmap/OffscreenCanvas.
5. Progress/cancel integration.
6. Diagnostics and UI badge.
7. Manual large-image validation.

This feature is a production-grade architectural step, not just an optimization: it keeps the Rust core reusable, supports huge images in the browser without filling the WASM heap, and creates the ingest abstraction that CLI/Tauri row-streaming decoders can use later.

## 11. Out of scope / future work

- npm publication of the processing client (the boundary is designed not to preclude it).
- Row-streaming decode in `r3sizer-io` for CLI/Tauri (the core push API already supports it).
- Tiling the content-adaptive resize path for huge images.
- Comparison demo, preset unification, README positioning vs pica — separate sub-projects from the same brainstorm, each needing its own spec.
