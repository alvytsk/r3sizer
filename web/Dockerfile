# syntax=docker/dockerfile:1
# Build context must be the repository root:
#   docker build -f web/Dockerfile -t r3sizer-web .

# ── Stage 1: Compile WASM bindings ──────────────────────────────────────────
FROM rust:1.94-bullseye AS wasm-builder

# Cache cargo registry and git checkouts across rebuilds
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo install wasm-pack

WORKDIR /workspace
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

# Cache the Rust build target directory; output goes to /wasm-out (not in target)
RUN --mount=type=cache,target=/workspace/target \
    wasm-pack build crates/r3sizer-wasm --target web --out-dir /wasm-out -- --features experimental

# Generate TypeScript type definitions from Rust types (ts-rs).
# Output goes to web/src/types/generated.ts which is copied in stage 2.
RUN --mount=type=cache,target=/workspace/target \
    mkdir -p web/src/types && \
    cargo test -p r3sizer-core --features typegen,experimental export_typescript_bindings -- --nocapture


# ── Stage 2: Build the React/Vite app ───────────────────────────────────────
FROM node:24-alpine AS web-builder

WORKDIR /app

# Install dependencies first for better layer caching
COPY web/package*.json ./
RUN --mount=type=cache,target=/root/.npm \
    npm ci

# Copy source, then inject artifacts built in stage 1
COPY web/ ./
COPY --from=wasm-builder /wasm-out ./src/wasm-pkg
COPY --from=wasm-builder /workspace/web/src/types/generated.ts ./src/types/generated.ts

# Run TypeScript check + Vite build (skip the build:wasm step; already done above)
RUN npx tsc -b && npx vite build


# ── Stage 3: Serve static files with nginx ──────────────────────────────────
FROM nginx:1.29-alpine AS runtime

COPY --from=web-builder /app/dist /usr/share/nginx/html/r3sizer
COPY web/nginx.conf /etc/nginx/conf.d/default.conf

EXPOSE 80

HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD wget -qO- http://localhost/ || exit 1

CMD ["nginx", "-g", "daemon off;"]
