# r3sizer architecture

This document describes the structure of r3sizer with C4-style views. It covers
four levels: system context, containers, shared components, and one runtime
sequence. It closes with the deployment topology.

For the algorithm the system implements, read [`algorithm.md`](algorithm.md).
For stage-by-stage internals, read
[`pipeline_implementation.md`](pipeline_implementation.md).

**Diagram notation.** The diagrams use Mermaid `flowchart` syntax rather than
Mermaid's native `C4Context` blocks. The native blocks lay out poorly on GitHub
and offer no layout control. The C4 level of each diagram is named in its
heading.

---

## Level 1: System context

r3sizer selects a sharpening strength for a downscaled image automatically. A
user reaches it through one of two surfaces. Both surfaces run the same
processing core.

```mermaid
flowchart TB
    photographer["<b>Photographer</b><br/><i>Person</i><br/>Resizes photos, one file<br/>or a whole directory"]
    developer["<b>Rust developer</b><br/><i>Person</i><br/>Embeds resizing in<br/>another application"]

    r3sizer["<b>r3sizer</b><br/><i>Software system</i><br/>Downscales images and<br/>selects the sharpening strength<br/>that meets an artifact budget"]

    fs[("<b>Local filesystem</b><br/><i>External</i><br/>Source and output<br/>image files")]
    browser["<b>Web browser</b><br/><i>External</i><br/>Hosts the WebAssembly<br/>build, no upload"]
    cratesio["<b>crates.io</b><br/><i>External</i><br/>Distributes the library<br/>and the CLI"]
    pages["<b>GitHub Pages</b><br/><i>External</i><br/>Serves the static<br/>web application"]

    photographer -->|"Runs commands,<br/>or drops a file in the browser"| r3sizer
    developer -->|"Calls the library API"| r3sizer
    r3sizer -->|"Reads and writes<br/>images and metadata"| fs
    r3sizer -->|"Runs entirely<br/>client-side in"| browser
    cratesio -.->|"Installs"| r3sizer
    pages -.->|"Hosts"| browser

    style r3sizer fill:#1168bd,stroke:#0b4884,color:#fff
    style photographer fill:#08427b,stroke:#052e56,color:#fff
    style developer fill:#08427b,stroke:#052e56,color:#fff
    style fs fill:#999,stroke:#6b6b6b,color:#fff
    style browser fill:#999,stroke:#6b6b6b,color:#fff
    style cratesio fill:#999,stroke:#6b6b6b,color:#fff
    style pages fill:#999,stroke:#6b6b6b,color:#fff
```

The system has no server component. It stores nothing and sends nothing over the
network. An image dropped into the web application never leaves the browser.

---

## Level 2: Containers

Four deployable or independently running units exist. The CLI is one process.
The web application is three units: a page and two kinds of Web Worker.

```mermaid
flowchart TB
    photographer["<b>Photographer</b><br/><i>Person</i>"]

    subgraph cli_boundary["CLI distribution"]
        cli["<b>r3sizer CLI</b><br/><i>Container: Rust binary, clap</i><br/>Single-file and batch processing,<br/>JSON diagnostics, preset inspection"]
    end

    subgraph web_boundary["Web application (browser)"]
        spa["<b>Diagnostic UI</b><br/><i>Container: React 19, Vite, Tailwind</i><br/>Decodes input, orchestrates workers,<br/>renders preview and diagnostics.<br/>State in Zustand"]
        mainworker["<b>Main WASM worker</b><br/><i>Container: Web Worker + wasm-bindgen</i><br/>Ingest, prepare_base, detail,<br/>fit, solve, final sharpen.<br/>Thread-local caches"]
        pool["<b>Probe worker pool</b><br/><i>Container: 1..6 Web Workers + wasm-bindgen</i><br/>Evaluates sharpening strengths<br/>in parallel. Caches base data"]
    end

    core[["<b>Shared Rust crates</b><br/><i>See Level 3</i>"]]
    fs[("<b>Local filesystem</b>")]

    photographer -->|"Commands"| cli
    photographer -->|"HTTPS, then all local"| spa

    cli --> core
    cli -->|"Reads, writes"| fs

    spa <-->|"postMessage,<br/>structured clone"| mainworker
    spa <-->|"postMessage,<br/>base data sent once"| pool
    mainworker --> core
    pool --> core

    style cli fill:#1168bd,stroke:#0b4884,color:#fff
    style spa fill:#1168bd,stroke:#0b4884,color:#fff
    style mainworker fill:#1168bd,stroke:#0b4884,color:#fff
    style pool fill:#1168bd,stroke:#0b4884,color:#fff
    style core fill:#438dd5,stroke:#2e6295,color:#fff
    style photographer fill:#08427b,stroke:#052e56,color:#fff
    style fs fill:#999,stroke:#6b6b6b,color:#fff
    style cli_boundary fill:none,stroke:#888,stroke-dasharray:4 4
    style web_boundary fill:none,stroke:#888,stroke-dasharray:4 4
```

The pool size is `min(navigator.hardwareConcurrency, 6)`. The pool is optional.
The UI falls back to the main worker alone when the pool fails to start or any
parallel step throws.

---

## Level 3: Components (the shared Rust crates)

Five crates make up the workspace. The dependency direction is strict and
enforced by the manifests.

```mermaid
flowchart TB
    cli["<b>r3sizer</b><br/><i>Component: CLI</i><br/>args, run, sweep, diff,<br/>corpus, presets, output"]
    wasm["<b>r3sizer-wasm</b><br/><i>Component: wasm-bindgen</i><br/>Ingest and probe exports,<br/>preserve_metadata"]
    io["<b>r3sizer-io</b><br/><i>Component: file I/O</i><br/>load_as_linear, save_from_linear,<br/>load_with_metadata, save_with_metadata"]
    meta["<b>r3sizer-metadata</b><br/><i>Component: metadata</i><br/>extract and merge for<br/>JPEG, PNG, WebP.<br/>No pixel decoding"]
    corec["<b>r3sizer-core</b><br/><i>Component: processing</i><br/>resize, classify, probe, fit,<br/>solve, sharpen. No I/O"]

    cli --> io
    cli --> corec
    io --> corec
    io --> meta
    wasm --> corec
    wasm --> meta
    corec -.->|"dev-only,<br/>typegen feature"| meta

    style corec fill:#438dd5,stroke:#2e6295,color:#fff
    style meta fill:#438dd5,stroke:#2e6295,color:#fff
    style io fill:#438dd5,stroke:#2e6295,color:#fff
    style cli fill:#85bbf0,stroke:#5d82a8,color:#000
    style wasm fill:#85bbf0,stroke:#5d82a8,color:#000
```

Two properties of this graph matter.

`r3sizer-core` performs no I/O. It accepts a `LinearRgbImage` and returns a
`ProcessOutput`. This lets the same code run in a CLI process, in a Web Worker,
and inside a future Tauri application without change.

The dashed edge is a development-only dependency behind the `typegen` feature.
It exists so the TypeScript exporter can emit the metadata contract types
alongside the core types into one `generated.ts`. A production build of
`r3sizer-core` never links metadata handling.

### Responsibilities

| Crate | Responsibility | Excluded by design |
|---|---|---|
| `r3sizer-core` | All pixel processing and strength selection | File access, encoding, metadata, async |
| `r3sizer-metadata` | EXIF, XMP, IPTC, ICC, text, density extract and merge | Pixel decoding, filesystem access |
| `r3sizer-io` | Decode, encode, and the metadata-aware load and save pair | Processing decisions |
| `r3sizer` | Argument parsing, batch orchestration, reporting | Processing decisions |
| `r3sizer-wasm` | The JavaScript boundary and thread-local caches | Processing decisions |

---

## Runtime view: interactive processing in the browser

This sequence shows the parallel two-pass path. It is the most involved flow in
the system. The CLI runs the same stages through the single-shot entry point
`process_auto_sharp_downscale`.

```mermaid
sequenceDiagram
    autonumber
    actor User
    participant UI as Diagnostic UI
    participant MW as Main WASM worker
    participant Pool as Probe worker pool

    User->>UI: Drop an image file
    UI->>UI: createImageBitmap, orientation from-image,<br/>sRGB canvas context

    alt Over 24 MP and shrink ratio at least 3x
        UI->>MW: ingest_begin, then ingest_stripe per stripe
        MW->>MW: StripedPreReducer accumulates into<br/>the 2x intermediate
        UI->>MW: ingest_end
    else Smaller image, or a modest downscale
        UI->>MW: prepare_image with the full RGBA buffer
    end

    UI->>MW: prepare_base(params)
    MW->>MW: Resize, classify regions, measure baseline
    MW-->>UI: PreparedBase cached, fingerprinted by BaseParamsKey

    par Fetch base and detail together
        UI->>MW: get_base_data
        and
        UI->>MW: compute_probe_detail(params)
        MW->>MW: One Gaussian blur, D = input - blur(input)
    end
    MW-->>UI: Base pixels plus the detail signal

    UI->>Pool: set_base once, cached in every worker
    UI->>MW: resolve_initial_strengths(params)
    MW-->>UI: Coarse strengths

    UI->>Pool: Round 1, coarse probes, distributed across workers
    Pool->>Pool: out = input + s * D, then measure.<br/>No blur per probe
    Pool-->>UI: Coarse samples

    UI->>MW: resolve_dense_strengths(coarse samples, effective P0)
    MW-->>UI: Dense window around the bracketed crossing

    UI->>Pool: Round 2, dense probes
    Pool-->>UI: Dense samples
    UI->>UI: Merge, sort by strength, deduplicate

    UI->>MW: process_from_probes(merged samples)
    MW->>MW: Fit cubic, run robustness checks, solve for s*,<br/>sharpen, chroma guard, clamp
    MW-->>UI: Output RGBA and AutoSharpDiagnostics
    UI-->>User: Preview and diagnostics

    opt User downloads the result
        UI->>MW: preserve_metadata(source bytes, encoded output)
        MW-->>UI: Merged bytes and a MetadataReport
        UI-->>User: File, plus a warning when the report has issues
    end
```

Three design points are visible in this sequence.

**The base is prepared once.** `prepare_base` costs roughly 1.5 seconds on a
24 MP image. `process_from_probes` costs roughly 0.5 seconds. A parameter change
that does not affect the base reuses the cached `PreparedBase`. The
`BaseParamsKey` fingerprint guards that reuse.

**The blur runs once, not once per probe.** The detail signal `D` does not depend
on the strength `s`. Each probe reduces to a multiply-add. This is why the base
data sent to the pool carries `detail`.

**Any parallel failure falls back.** `processImageParallel` catches every error
except cancellation and reruns the whole request on the main worker.

---

## Deployment

```mermaid
flowchart TB
    subgraph dev["Developer machine"]
        repo["Git repository"]
    end

    subgraph gha["GitHub Actions"]
        direction TB
        ci["<b>ci.yml</b><br/>fmt, clippy -D warnings,<br/>workspace tests, cargo doc,<br/>ts-rs freshness, wasm-pack test"]
        audit["<b>audit.yml</b><br/>Weekly cargo audit,<br/>and on any Cargo.lock change"]
        deployjob["<b>deploy.yml</b><br/>Rust plus wasm32 target,<br/>wasm-pack, npm ci, npm run build"]
    end

    subgraph runtime["Runtime targets"]
        pages["<b>GitHub Pages</b><br/>Static assets from web/dist<br/>alvytsk.github.io/r3sizer"]
        crates["<b>crates.io</b><br/>r3sizer-core, r3sizer-io,<br/>r3sizer-metadata, r3sizer"]
        local["<b>User machine</b><br/>r3sizer binary"]
    end

    repo -->|"Push or PR"| ci
    repo -->|"Push or PR"| audit
    repo -->|"Push to main"| deployjob
    deployjob -->|"upload-pages-artifact,<br/>deploy-pages"| pages
    repo -->|"Manual, tagged,<br/>in dependency order"| crates
    crates -->|"cargo install r3sizer"| local

    style ci fill:#1168bd,stroke:#0b4884,color:#fff
    style audit fill:#1168bd,stroke:#0b4884,color:#fff
    style deployjob fill:#1168bd,stroke:#0b4884,color:#fff
    style pages fill:#438dd5,stroke:#2e6295,color:#fff
    style crates fill:#438dd5,stroke:#2e6295,color:#fff
    style local fill:#438dd5,stroke:#2e6295,color:#fff
    style dev fill:none,stroke:#888,stroke-dasharray:4 4
    style gha fill:none,stroke:#888,stroke-dasharray:4 4
    style runtime fill:none,stroke:#888,stroke-dasharray:4 4
```

The web build runs entirely in CI. No local artifact is committed. The WebAssembly
module is built from source in the same job that builds the page.

Crate publishing is manual. There is no release workflow. Publish the crates in
dependency order. Read [`releasing.md`](releasing.md) for the procedure.

`r3sizer-wasm` sets `publish = false`. It ships only inside the web bundle.

---

## Cross-cutting constraints

| Constraint | Where it is enforced | Why |
|---|---|---|
| Pixels in `f32`, polynomial fit in `f64` | `fit.rs` | The Vandermonde matrix carries terms up to `s^6`. `f32` causes catastrophic cancellation |
| No clamping inside `sharpen.rs` | `sharpen.rs` | Out-of-range values are the artifact signal. Clamping happens once, at output |
| The downscaled base is never mutated during probing | `pipeline.rs` | Each probe allocates fresh output, so the base stays valid for the final apply |
| A fallback is not an error | `solve.rs` | When no root falls in the probe range, the best sample is used. The pipeline always returns a result, and reports how it chose |
| TypeScript types are generated, never handwritten | `ts-rs`, `typegen` feature | CI regenerates `generated.ts` and fails when the commit is stale |
| Metadata issues are reported, never silently dropped | `r3sizer-metadata` | The system never claims preservation it cannot verify |
| Peak memory is bounded for very large inputs | `ingest.rs` | Striped ingest holds one stripe plus the intermediate, not the full source |

---

## Where to read next

| Topic | Document |
|---|---|
| The selection algorithm, stage by stage | [`algorithm.md`](algorithm.md) |
| Data flow, allocations, and fallback logic | [`pipeline_implementation.md`](pipeline_implementation.md) |
| Confirmed findings against engineering approximations | [`assumptions.md`](assumptions.md) |
| Every CLI flag | [`cli.md`](cli.md) |
| Roadmap | [`future_work.md`](future_work.md) |
| Version, tag, and publish procedure | [`releasing.md`](releasing.md) |
