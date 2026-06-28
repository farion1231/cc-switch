# =============================================================================
# Stage 1: Build web resources (frontend)
# =============================================================================
FROM node:22-bookworm AS web-builder

# Use Aliyun mirror for apt sources
RUN sed -i 's|deb.debian.org|mirrors.aliyun.com|g; s|security.debian.org|mirrors.aliyun.com|g' /etc/apt/sources.list.d/debian.sources

WORKDIR /app
RUN npm install -g pnpm@10.30.1 --registry=https://registry.npmmirror.com
RUN npm config set registry https://registry.npmmirror.com
RUN pnpm config set registry https://registry.npmmirror.com

# Copy dependency manifests first (layer-cached: only reinstalls when these change)
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY tsconfig.json tsconfig.node.json vite.config.ts postcss.config.cjs tailwind.config.cjs components.json ./
COPY src/index.html ./index.html
COPY src ./src
COPY assets ./assets
# The login page imports the desktop icon through the @tauri-icons alias.
COPY src-tauri/icons/icon.png ./src-tauri/icons/icon.png

RUN pnpm install --no-frozen-lockfile
RUN pnpm build:renderer

# =============================================================================
# Stage 2: Build Rust backend
# =============================================================================
FROM rust:1.90-bookworm

# Build-time configuration
ARG CARGO_FEATURES=""

# Use Aliyun mirror for apt sources
RUN sed -i 's|deb.debian.org|mirrors.aliyun.com|g; s|security.debian.org|mirrors.aliyun.com|g' /etc/apt/sources.list.d/debian.sources

# Install system dependencies (layer-cached unless this block changes)
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       pkg-config \
       libssl-dev \
       libgtk-3-dev \
       libwebkit2gtk-4.1-dev \
       libayatana-appindicator3-dev \
       librsvg2-dev \
       ca-certificates \
       xvfb \
       xauth \
    && rm -rf /var/lib/apt/lists/*

# Configure Cargo to use Aliyun mirror (sparse protocol = faster index fetches)
RUN mkdir -p /usr/local/cargo \
        && printf '%s\n' \
            '[source.crates-io]' \
            'replace-with = "aliyun"' \
            '[source.aliyun]' \
            'registry = "sparse+https://mirrors.aliyun.com/crates.io-index/"' \
            '[net]' \
            'git-fetch-with-cli = true' \
            > /usr/local/cargo/config.toml

# Override the slow release profile for faster Docker builds
# codegen-units=16: parallel LLVM codegen (up from 1 — biggest speed win)
# lto=off: skip link-time optimization
# opt-level=2: faster compilation than opt-level=s
ENV RUSTFLAGS="-C codegen-units=16 -C lto=off -C opt-level=2"

WORKDIR /app

# Install cargo-chef for dependency caching
RUN cargo install cargo-chef --locked

# -------------------------------------------------------------------
# Dependency layer — only rebuilds when Cargo.toml or Cargo.lock change
# -------------------------------------------------------------------
COPY src-tauri/Cargo.toml src-tauri/Cargo.lock ./src-tauri/

# Create dummy source files so cargo can resolve the crate for metadata
RUN mkdir -p src-tauri/src \
    && echo 'fn main() {}' > src-tauri/src/main.rs \
    && echo '' > src-tauri/src/lib.rs

# cargo-chef expects Cargo.toml in the current directory (no --manifest-path support)
WORKDIR /app/src-tauri

# Capture the full dependency graph from the manifests
RUN cargo chef prepare --recipe-path recipe.json

# Build ALL dependencies once (cache mount persists compiled crates across builds)
# This step is the biggest time saver — it's cached unless Cargo.toml/Cargo.lock change
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/src-tauri/target \
    cargo chef cook --release --recipe-path recipe.json

# -------------------------------------------------------------------
# Application layer — only rebuilds when your source code changes
# -------------------------------------------------------------------
WORKDIR /app
COPY src-tauri/ ./src-tauri/
COPY --from=web-builder /app/dist ./src-tauri/web-dist

WORKDIR /app/src-tauri
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/src-tauri/target \
    if [ -n "$CARGO_FEATURES" ]; then \
        cargo build --release --features "$CARGO_FEATURES"; \
    else \
        cargo build --release; \
    fi \
    && cp target/release/cc-switch /usr/local/bin/cc-switch

WORKDIR /app
EXPOSE 3001
