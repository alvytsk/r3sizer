# r3sizer Performance Optimization Plan

## Goal

Reduce end-to-end pipeline latency in `r3sizer` while preserving the project’s core identity:

* deterministic behavior
* strong diagnostics
* artifact-limited sharpness selection
* linear-light processing
* modular Rust-first architecture

This plan is written for the current project direction: **production-grade Rust core first**, optional CLI/WASM/Tauri integration later.

---

## Executive summary

The highest-value performance work for `r3sizer` is **not** low-level micro-optimization first. The main cost center is likely the **multi-probe parameter selection loop**, especially when every probe performs a full sharpen + reconstruct + metric pass over the output image.

The recommended order is:

1. **Reduce probe work**
2. **Reuse more intermediates**
3. **Parallelize probe evaluation**
4. **Use staged shrinking for large reductions**
5. **Introduce fast / balanced / quality runtime modes**
6. **Only then invest in deeper algorithmic or ML-inspired acceleration**

This prioritization is also consistent with current image-library practice. libvips explicitly speeds downsizing with a box-filter shrink before the final fair resample via its `gap` parameter, while recent adaptive resampling work such as **LeRF** shows that local adaptivity can be accelerated with LUT-style strategies rather than paying full model cost at every pixel. [libvips resize docs](https://www.libvips.org/API/current/method.Image.resize.html), [LeRF 2025](https://pubmed.ncbi.nlm.nih.gov/40478709/)

---

## Assumptions about the current pipeline

Current baseline shape:

1. Load image
2. Convert sRGB-like input to linear RGB
3. Downscale in linear RGB
4. Optional contrast leveling
5. Build luminance / analysis context
6. Probe multiple sharpening strengths
7. Compute artifact metric `P(s)` for each probe
8. Fit cubic and solve for `s*`
9. Apply final sharpening
10. Clamp / convert / save

Probable hot spots:

* resize/downscale stage
* per-probe sharpen + RGB reconstruction
* per-probe metric evaluation
* repeated allocation / temporary image creation
* adaptive region classification / gain-map generation if always enabled

---

## Optimization roadmap

# Phase 1 — Highest ROI, low algorithmic risk

## 1. Reduce probe count and add early stopping

### What to change

Introduce explicit probing policies:

* `fast`: 3–4 probes total
* `balanced`: 4–6 probes total
* `quality`: current richer probing
* `research`: unrestricted / diagnostic-heavy mode

Add early-stop conditions:

* all tested strengths safely below budget
* all tested strengths above budget
* target crossing already bracketed tightly enough
* fit quality already sufficient and refinement is unlikely to change output materially

### Why this matters

The selection loop is one of the few parts that repeats a near-full image pass several times. Reducing probes often gives larger wins than optimizing one inner kernel.

### Expected impact

* **Latency impact:** High
* **Expected speedup:** ~`1.3x` to `2.5x` end-to-end depending on current probe count and image size
* **Memory impact:** Neutral

### Difficulty

* **Implementation difficulty:** Low to Medium

### Risk

* **Algorithmic risk:** Medium
* Risk is mainly output drift if probe budgets become too aggressive on hard images

### Risk mitigation

* keep current policy as `quality` or `research`
* compare `fast/balanced` vs current on a benchmark corpus
* report diagnostic flag when result was chosen under reduced probing

---

## 2. Reuse all strength-independent intermediates

### What to change

Create a prepared analysis context that is built once per image:

* resized linear RGB base
* luminance extraction
* optional baseline RGB↔luma support buffers
* region map / gain map
* baseline artifact measurement
* scratch-buffer pools

Suggested types:

* `PreparedImage`
* `PreparedAutoSharpContext`
* `ProbeScratch`

### Why this matters

Anything that does not depend on sharpen strength should not be recomputed per probe.

### Expected impact

* **Latency impact:** High
* **Expected speedup:** ~`1.2x` to `1.8x` end-to-end
* **Memory impact:** Slightly positive if allocations are reduced and scratch buffers are reused

### Difficulty

* **Implementation difficulty:** Medium

### Risk

* **Algorithmic risk:** Low
* **Engineering risk:** Medium due to lifetime / mutability / buffer ownership design in Rust

### Risk mitigation

* keep immutable prepared state and thread-local mutable scratch
* benchmark allocation count before/after

---

## 3. Parallelize probe evaluation across strengths

### What to change

Use Rayon or equivalent CPU parallelism to evaluate probe strengths concurrently.

Per-probe work is naturally parallel if the shared base image is read-only.

### Why this matters

Probe evaluations are embarrassingly parallel. On multi-core desktop CPUs, this can reduce wall-clock time substantially.

### Expected impact

* **Latency impact:** High on multi-core CPUs
* **Expected speedup:** ~`1.5x` to `4x` wall-clock for probe stage depending on core count and memory bandwidth
* **Memory impact:** Higher temporary memory unless scratch is carefully managed

### Difficulty

* **Implementation difficulty:** Medium

### Risk

* **Algorithmic risk:** Low
* **Engineering risk:** Medium due to memory pressure and cache behavior

### Risk mitigation

* use shared immutable prepared state
* allocate thread-local scratch only once per worker
* cap parallelism when image is small or memory pressure is high

---

## 4. Add staged shrinking before final high-quality reduce

### What to change

For large shrink factors, add a two-stage classical resize path:

1. cheap pre-reduce (integer / near-integer box-like shrink)
2. final fair resample in linear RGB

This follows the same broad idea as libvips `gap`, where a box-filter shrink is used first to speed downscaling while remaining close to fair resampling. [libvips resize docs](https://www.libvips.org/API/current/method.Image.resize.html)

### Why this matters

Downscaling from large inputs to much smaller outputs pays for expensive filtering over too many pixels if performed in one heavy stage.

### Expected impact

* **Latency impact:** High for large shrink ratios
* **Expected speedup:** ~`1.2x` to `3x` in resize-heavy cases
* **Memory impact:** Neutral to slightly positive

### Difficulty

* **Implementation difficulty:** Medium

### Risk

* **Algorithmic risk:** Medium
* Can subtly alter final image character if pre-reduce is too aggressive

### Risk mitigation

* enable only above a threshold ratio, for example > `2.5x` or `3x`
* benchmark against single-stage baseline on difficult images
* expose a policy knob

---

# Phase 2 — Strong next wave

## 5. Add runtime modes: fast / balanced / quality

### What to change

Introduce a top-level performance-quality control that changes:

* probe budget
* metric richness
* adaptive-region precision
* staged shrink aggressiveness
* diagnostics depth

### Why this matters

Different users want different tradeoffs. This also keeps optimization changes controlled and understandable.

### Expected impact

* **Latency impact:** Medium to High depending on mode
* **Expected speedup:** `fast` mode could plausibly be ~`2x` or more vs current conservative baseline

### Difficulty

* **Implementation difficulty:** Low to Medium

### Risk

* **Algorithmic risk:** Low if quality mode preserves current behavior

---

## 6. Tile-based or proxy-based probing

### What to change

Instead of evaluating every probe on the full output image, evaluate on:

* selected high-information tiles
* representative edge / texture / saturation zones
* optionally a proxy-resolution output for parameter search

Then apply the chosen strength to the full image once.

### Why this matters

The strength solver may not need full-image evaluation for every candidate.

### Expected impact

* **Latency impact:** High
* **Expected speedup:** ~`1.5x` to `3x` for probing-heavy workloads

### Difficulty

* **Implementation difficulty:** Medium to High

### Risk

* **Algorithmic risk:** Medium to High
* Probe estimate may disagree with full-image behavior, especially on unusual images

### Risk mitigation

n

* prefer tile-based probing over pure proxy-resolution probing
* choose tiles by region classifier / edge density / saturation risk
* validate on a stress corpus

---

## 7. Make metric evaluation multi-stage

### What to change

Use a cheap-first metric pipeline:

1. cheap guard metric
2. richer composite metrics only near likely crossing region or final candidates

For example:

* always compute gamut excursion
* compute halo/overshoot/alias proxies only for shortlisted strengths or risky regions

### Why this matters

Composite metrics are useful, but if computed at full cost for every probe they can erase performance gains.

### Expected impact

* **Latency impact:** Medium
* **Expected speedup:** ~`1.1x` to `1.6x` depending on metric complexity

### Difficulty

* **Implementation difficulty:** Medium

### Risk

* **Algorithmic risk:** Medium if shortlist logic misses edge cases

---

## 8. Add lightweight adaptive mode

### What to change

If adaptive sharpening / gain maps are expensive, add tiers:

* `off`
* `light`: tile-level or simplified region classes
* `full`: current / future pixel-level adaptive mode

### Why this matters

Adaptivity is valuable, but dense per-pixel analysis may not always be worth the cost.

### Expected impact

* **Latency impact:** Medium
* **Expected speedup:** ~`1.1x` to `1.8x` depending on current adaptive cost

### Difficulty

* **Implementation difficulty:** Medium

### Risk

* **Algorithmic risk:** Low to Medium

---

# Phase 3 — Deeper engineering / research work

## 9. Precompute reusable sharpen detail if mathematically valid

### What to change

If sharpen behavior can be expressed approximately as:

`out = base + s * detail`

then compute `detail` once and reuse it for all probe strengths.

### Why this matters

This can collapse probe cost dramatically.

### Expected impact

* **Latency impact:** Very High
* **Expected speedup:** potentially huge for probe stage, often larger than `2x`

### Difficulty

* **Implementation difficulty:** Medium to High

### Risk

* **Algorithmic risk:** High unless the sharpen operator is sufficiently linear in `s`

### Recommendation

Treat as a research/engineering experiment, not immediate baseline work.

---

## 10. SIMD and cache-oriented kernel optimization

### What to change

Focus on hot loops:

* luminance extraction
* separable blur passes
* RGB reconstruction
* metric accumulation
* clamp/convert

Use:

* contiguous memory layouts
* fewer branches in inner loops
* possible SIMD via Rust intrinsics / crates where stable and worthwhile

### Expected impact

* **Latency impact:** Medium
* **Expected speedup:** ~`1.1x` to `1.5x` after bigger structural wins are done

### Difficulty

* **Implementation difficulty:** Medium to High

### Risk

* **Algorithmic risk:** Low
* **Engineering risk:** Medium due to portability and maintenance

---

## 11. LUT-driven adaptive resampling experiments

### What to change

Investigate lookup-table driven adaptive resampling inspired by recent work such as LeRF, where local resampling behavior is accelerated using LUTs. The project implication is **not** “ship a neural resizer,” but “consider a fast local-kernel-selection architecture with precomputed responses.” [LeRF 2025](https://pubmed.ncbi.nlm.nih.gov/40478709/)

### Expected impact

* **Latency impact:** Unknown initially
* **Potential impact:** high if it yields better quality-speed tradeoffs in adaptive paths

### Difficulty

* **Implementation difficulty:** High

### Risk

* **Algorithmic risk:** Medium to High
* **Project-scope risk:** High if it distracts from shipping the classical core

### Recommendation

Keep out of the baseline roadmap until the classical pipeline is already well-profiled and stable.

---

# Recommended implementation order

## Immediate backlog — COMPLETED (v0.7)

1. ~~Probe policy reduction + early stopping~~ — `PipelineMode` + early-stop in coarse scan
2. ~~Prepared analysis context + scratch reuse~~ — `PreparedBase` + detail precomputation
3. ~~Parallel probe execution~~ — rayon (native) + Web Worker pool (WASM)
4. ~~Runtime modes (`fast/balanced/quality`)~~ — `PipelineMode` enum with `resolved()`
5. ~~Staged shrink path for large reductions~~ — bilinear pre-reduce at >= 3x ratio
6. ~~Reusable sharpen-detail formulation~~ — `compute_probe_detail` / `run_probes_from_detail`
7. ~~SIMD resize~~ — switched to `fast_image_resize` crate (SSE4.1/AVX2)
8. ~~Source-side diagnostics skip~~ — `base_quality::score_base_resize(full_diagnostics: bool)`

**Measured results (1024x768 -> 512x384, release, x86-64):**
* Fast mode: **34 ms** end-to-end
* Balanced mode: **117 ms** end-to-end
* Quality mode: **139 ms** end-to-end
* Resize stage: **7.7 ms** (was 31 ms with `image` crate)
* Probing (7+4 TwoPass): **6-13 ms** (detail precomputation eliminates per-probe blur)

## Next backlog

6. Multi-stage metric evaluation — partially done (fast path exists, no guard metric)
7. Lightweight adaptive mode — not started
8. Tile-based probing — not started

## Research backlog

9. ~~Reusable sharpen-detail formulation~~ — DONE (moved to immediate)
10. ~~SIMD pass tuning~~ — DONE for resize; Gaussian blur still hand-rolled
11. LUT/adaptive resampling experiments — not started

---

## Suggested profiling plan before coding

Before major refactors, instrument and measure:

* total resize time
* total probe time
* time per probe
* final sharpen time
* metric evaluation time
* allocations / reallocations per image
* peak RSS / scratch-buffer footprint
* speed split by image size and shrink ratio

Suggested diagnostic fields:

* `timings.resize_ms`
* `timings.analysis_ms`
* `timings.probe_total_ms`
* `timings.probe_avg_ms`
* `timings.fit_ms`
* `timings.final_sharpen_ms`
* `timings.encode_ms`
* `probe_count`
* `parallel_probe_workers`
* `used_staged_shrink`
* `used_fast_mode`

This should be added before or alongside optimization work so performance wins are measurable and regressions are obvious.

---

## Proposed acceptance criteria

### Phase 1 acceptance

* `balanced` mode preserves visual parity with current baseline on the validation corpus
* median runtime improvement at least `30–50%` on large images
* no increase in fit/solver instability beyond agreed threshold
* diagnostics clearly report reduced-probing or staged-shrink usage

### Phase 2 acceptance

* tile-based probing error remains within acceptable tolerance versus full-image probing
* lightweight adaptive mode gives measurable quality retention at lower cost

### Phase 3 acceptance

* research optimizations remain behind feature flags until validated

---

## Bottom-line recommendation

If only one optimization batch is implemented next, it should be:

1. **fewer probes**
2. **early stop**
3. **reuse intermediates**
4. **parallel probe evaluation**

That combination is the best expected speedup-per-effort package and preserves the algorithmic identity of `r3sizer`.

The second best structural win is a **libvips-style staged shrink path** for large downscales, because this is a proven classical technique for improving quality-speed tradeoffs in real image libraries. [libvips resize docs](https://www.libvips.org/API/current/method.Image.resize.html)

The main thing to avoid is prematurely jumping to a neural or research-heavy acceleration path before the current classical pipeline has been fully profiled and simplified.
