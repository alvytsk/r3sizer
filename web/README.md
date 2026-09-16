# r3sizer — web client

React + TypeScript + Vite frontend for the r3sizer image-processing pipeline.
Calls into the core algorithm via a WebAssembly module compiled from `crates/r3sizer-wasm`,
which also links `crates/r3sizer-metadata` to preserve embedded image metadata
(EXIF/XMP/IPTC/ICC/text/density) through exports — see [Metadata preservation](#metadata-preservation).

## Prerequisites

| Tool | Minimum version | Purpose |
|------|----------------|---------|
| Node.js | 22 | JS runtime & npm |
| Rust toolchain | stable | WASM compilation |
| [wasm-pack](https://rustwasm.github.io/wasm-pack/) | 0.13 | Build WASM bindings |

Install wasm-pack:
```sh
cargo install wasm-pack
```

## Local development

Run from the `web/` directory.

```sh
# 1. Build the WASM package (required once, and after core changes)
npm run build:wasm

# 2. Start the dev server with HMR
npm run dev
```

The dev server starts at `http://localhost:5173`.

Re-run `build:wasm` whenever you change anything under `crates/r3sizer-wasm` or `crates/r3sizer-core`.

## Build

```sh
# Full production build (WASM + TypeScript + Vite)
npm run build

# Preview the production build locally
npm run preview
```

Output is written to `web/dist/`.

## Docker

Build and run the web client as a self-contained nginx container.
Run the following commands from the **repository root** (the build context must include the Rust crates):

```sh
# Build the image
docker build -f web/Dockerfile -t r3sizer-web .

# Run on port 8080
docker run --rm -p 8080:80 r3sizer-web
```

Then open `http://localhost:8080`.

### Build stages

| Stage | Base image | What it does |
|-------|-----------|--------------|
| `wasm-builder` | `rust:1` | Compiles `crates/r3sizer-wasm` with wasm-pack |
| `web-builder` | `node:22-alpine` | Runs TypeScript + Vite build |
| `runtime` | `nginx:1.27-alpine` | Serves `dist/` as static files |

BuildKit cache mounts are used for the Cargo registry and npm cache, so incremental builds are fast.

## Lint

```sh
npm run lint
```

## Metadata preservation

Downloading a processed image preserves its embedded metadata (EXIF, XMP,
IPTC, ICC, text, density) by default for JPEG/PNG/WebP — no companion files;
metadata is embedded in the downloaded image itself. Each output stays bound
to the source `File` it came from, so export always extracts metadata from
the correct original.

When metadata can't be fully carried over (unsupported source/destination
combination, malformed metadata, an unverifiable color profile, limits
exceeded), the download still succeeds and a visible, localized
`role="status"` warning is shown — the export never silently drops metadata
without telling you, and a worker/extraction failure falls back to the
original encoded output rather than blocking the download. This is not a
complete enumeration of every possible metadata field or loss case; see
[`crates/r3sizer-metadata`](../crates/r3sizer-metadata/README.md) for the
full capability/limit list, and
[`docs/testing/metadata-export.md`](../docs/testing/metadata-export.md) for
the browser verification recipe and observed results.

Ingestion decodes images with `imageOrientation: "from-image"` (orientation
already applied to the pixels shown/exported) and requests sRGB canvas
contexts throughout; export reports `color: "unverified"` unless the canvas
context's own reported color space actually confirms sRGB, rather than
assuming it.
