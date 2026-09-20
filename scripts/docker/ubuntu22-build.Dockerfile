FROM ubuntu:22.04

ARG DEBIAN_FRONTEND=noninteractive
ARG UID=1000
ARG GID=1000
ARG NODE_VERSION=22.12.0
ARG PNPM_VERSION=10.12.3
ARG RUST_TOOLCHAIN=1.95.0

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    git \
    xz-utils \
    build-essential \
    pkg-config \
    file \
    patchelf \
    libglib2.0-dev \
    libssl-dev \
    libgtk-3-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    libwebkit2gtk-4.1-dev \
    libjavascriptcoregtk-4.1-dev \
    libsoup-3.0-dev \
    libxdo-dev \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd -g ${GID} builder \
    && useradd -m -u ${UID} -g ${GID} -s /bin/bash builder

RUN curl -fsSLo /opt/node.tar.xz "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-x64.tar.xz" \
    && tar -xJf /opt/node.tar.xz -C /opt \
    && ln -s "/opt/node-v${NODE_VERSION}-linux-x64/bin/node" /usr/local/bin/node \
    && ln -s "/opt/node-v${NODE_VERSION}-linux-x64/bin/npm" /usr/local/bin/npm \
    && ln -s "/opt/node-v${NODE_VERSION}-linux-x64/bin/npx" /usr/local/bin/npx \
    && ln -s "/opt/node-v${NODE_VERSION}-linux-x64/bin/corepack" /usr/local/bin/corepack \
    && rm -f /opt/node.tar.xz

ENV PATH="/opt/node-v${NODE_VERSION}-linux-x64/bin:/home/builder/.cargo/bin:${PATH}"

RUN npm install -g "pnpm@${PNPM_VERSION}"

USER builder
WORKDIR /workspace

RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --default-toolchain ${RUST_TOOLCHAIN} --profile minimal \
    && rustup component add rustfmt clippy

CMD ["bash"]
