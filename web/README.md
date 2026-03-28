# r3sizer — web client

React + TypeScript + Vite frontend for the r3sizer image-processing pipeline.
Calls into the core algorithm via a WebAssembly module compiled from `crates/r3sizer-wasm`.

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
