#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE_NAME="${IMAGE_NAME:-cc-switch-ubuntu22-build}"
IMAGE_TAG="${IMAGE_TAG:-local}"
IMAGE="${IMAGE_NAME}:${IMAGE_TAG}"
DOCKERFILE_PATH="${ROOT_DIR}/scripts/docker/ubuntu22-build.Dockerfile"

CACHE_DIR="${ROOT_DIR}/.cache/ubuntu22-build"
mkdir -p "${CACHE_DIR}/pnpm-store" "${CACHE_DIR}/npm"

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is required but not found" >&2
  exit 1
fi

if ! docker info >/dev/null 2>&1; then
  echo "docker daemon is not available for current user" >&2
  exit 1
fi

build_image() {
  docker build \
    --build-arg UID="$(id -u)" \
    --build-arg GID="$(id -g)" \
    --tag "${IMAGE}" \
    --file "${DOCKERFILE_PATH}" \
    "${ROOT_DIR}"
}

run_container() {
  docker run --rm -t \
    -v "${ROOT_DIR}:/workspace" \
    -v "${CACHE_DIR}/pnpm-store:/home/builder/.local/share/pnpm/store" \
    -v "${CACHE_DIR}/npm:/home/builder/.npm" \
    -w /workspace \
    "${IMAGE}" \
    bash -lc "$*"
}

build_deb() {
  run_container 'set -euo pipefail
    CI=1 pnpm install --frozen-lockfile
    pnpm tauri build --bundles deb --config "{\"bundle\":{\"createUpdaterArtifacts\":false}}"'
}

cmd="${1:-}"

case "${cmd}" in
  build-image)
    build_image
    ;;
  validate)
    build_image
    run_container 'set -euo pipefail
      cat /etc/os-release | grep PRETTY_NAME
      node -v
      pnpm -v
      rustc -V
      cargo -V
      echo "pkg-config glib-2.0: $(pkg-config --modversion glib-2.0 || echo missing)"
      echo "pkg-config javascriptcoregtk-4.1: $(pkg-config --modversion javascriptcoregtk-4.1 || echo missing)"
      echo "pkg-config libsoup-3.0: $(pkg-config --modversion libsoup-3.0 || echo missing)"
      CI=1 pnpm install --frozen-lockfile
      pnpm typecheck
      cargo check --manifest-path src-tauri/Cargo.toml'
    ;;
  build-deb)
    build_image
    build_deb
    ;;
  build)
    build_image
    build_deb
    ;;
  shell)
    build_image
    docker run --rm -it \
      -v "${ROOT_DIR}:/workspace" \
      -v "${CACHE_DIR}/pnpm-store:/home/builder/.local/share/pnpm/store" \
      -v "${CACHE_DIR}/npm:/home/builder/.npm" \
      -w /workspace \
      "${IMAGE}" \
      bash
    ;;
  *)
    cat >&2 <<USAGE
Usage: scripts/ubuntu22-build-env.sh <command>

Commands:
  build-image  Build local Ubuntu 22.04 build image
  validate     Validate toolchain + dependency install + typecheck + cargo check
  build-deb    Build Linux .deb bundle in Ubuntu 22.04 image
  build        Alias of build-deb
  shell        Open interactive shell in Ubuntu 22.04 build image
USAGE
    exit 1
    ;;
esac
