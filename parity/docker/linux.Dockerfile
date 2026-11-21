# Parity suite Linux lane image: build and run a parity build of CC Switch under Xvfb with WebKitWebDriver.
# Base pinned by digest (ubuntu 22.04, linux/amd64), the same release CI's backend job uses.
# A host whose Docker daemon cannot reach the registry loads that digest with crane and passes
# its local tag through BASE.
ARG BASE=ubuntu@sha256:b682e31336cc8e0a73ed91ab32b7a95e32591c5ff09994490a20accf9b3600f9
FROM ${BASE}

ARG DEBIAN_FRONTEND=noninteractive
ARG NODE_VERSION=22.22.2
ARG PNPM_VERSION=10.12.3
ARG RUST_VERSION=1.95
ARG TAURI_DRIVER_VERSION=2.1.0

RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl git xz-utils build-essential pkg-config libssl-dev \
      libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev libwebkit2gtk-4.1-dev libsoup-3.0-dev \
      webkit2gtk-driver xvfb xauth dbus dbus-x11 procps x11-apps \
      fonts-dejavu-core fonts-noto-cjk fonts-noto-color-emoji \
    && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-x64.tar.xz" \
      | tar -xJ -C /usr/local --strip-components=1 \
    && npm install -g "pnpm@${PNPM_VERSION}"

ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo PATH=/usr/local/cargo/bin:$PATH
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain "${RUST_VERSION}" --no-modify-path \
    && cargo install tauri-driver --version "${TAURI_DRIVER_VERSION}" --locked \
    && rm -rf /usr/local/cargo/registry /usr/local/cargo/git \
    && chmod -R a+rwX /usr/local/cargo /usr/local/rustup

# The app reads TZ and LANG. Runs set HOME per instance.
ENV TZ=UTC LANG=en_US.UTF-8 LC_ALL=C.UTF-8
