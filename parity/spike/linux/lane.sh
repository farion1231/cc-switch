#!/usr/bin/env bash
# Parity suite Linux lane.
#
#   parity/spike/linux/lane.sh build   build a parity binary of this checkout inside the image
#   parity/spike/linux/lane.sh probe   run the probe on it, offline, with a fresh HOME per launch
#
# Build runs need the network (crates, npm). Probe runs get none. Docker's client-side proxy
# settings are cleared in both, so nothing the app does can reach a proxy by accident.
set -euo pipefail

REPO=$(cd "$(dirname "$0")/../../.." && pwd)
IMAGE=${PARITY_IMAGE:-ccs-parity-linux:dev}
CACHE=${PARITY_CACHE:-$HOME/.cache/ccs-parity}
OUT=${PARITY_OUT:-$REPO/parity/.out/linux}
RUNS=${PARITY_RUNS:-7}
mkdir -p "$CACHE/target" "$CACHE/cargo" "$OUT"

no_proxy_env=()
for v in HTTP_PROXY HTTPS_PROXY ALL_PROXY NO_PROXY FTP_PROXY http_proxy https_proxy all_proxy no_proxy ftp_proxy; do
  no_proxy_env+=(-e "$v=")
done

case "${1:-}" in
  build)
    build_proxy=()
    if [[ -n "${PARITY_BUILD_PROXY:-}" ]]; then
      build_proxy=(-e "http_proxy=$PARITY_BUILD_PROXY" -e "https_proxy=$PARITY_BUILD_PROXY"
                   -e "HTTP_PROXY=$PARITY_BUILD_PROXY" -e "HTTPS_PROXY=$PARITY_BUILD_PROXY")
    fi
    mkdir -p "$CACHE/pnpm-store"
    docker run --rm --network host "${no_proxy_env[@]}" "${build_proxy[@]}" \
      --user "$(id -u):$(id -g)" -e HOME=/tmp/build-home \
      -e CARGO_TARGET_DIR=/target -e CARGO_HOME=/cargo-home -e npm_config_store_dir=/pnpm-store \
      -v "$CACHE/target:/target" -v "$CACHE/cargo:/cargo-home" -v "$CACHE/pnpm-store:/pnpm-store" \
      -v "$REPO:/work" -v "$OUT:/out" -w /work \
      "$IMAGE" bash -c '
        set -e
        mkdir -p "$HOME"
        [ -d node_modules ] || pnpm install --frozen-lockfile
        pnpm tauri build --no-bundle --features parity
        cp /target/release/cc-switch /out/cc-switch'
    ;;
  probe)
    test -x "$OUT/cc-switch" || { echo "no binary at $OUT/cc-switch; run '$0 build' first" >&2; exit 2; }
    # Root inside the container, because dbus needs a passwd entry; outputs are handed back to the caller.
    docker run --rm --network none "${no_proxy_env[@]}" \
      -e HOME=/tmp/sentinel-home -e XDG_RUNTIME_DIR=/tmp/xdg-runtime -e OWNER="$(id -u):$(id -g)" \
      -v "$OUT/cc-switch:/app/cc-switch:ro" -v "$REPO/parity/spike:/spike:ro" -v "$OUT:/out" \
      "$IMAGE" bash -c '
        set -e
        trap "chown -R \$OWNER /out" EXIT
        mkdir -p "$HOME" && mkdir -m 700 -p "$XDG_RUNTIME_DIR"
        env | grep -i proxy | grep -v "=$" && { echo "proxy variable leaked into the container" >&2; exit 3; }
        Xvfb :99 -screen 0 1280x800x24 -nolisten tcp >/tmp/xvfb.log 2>&1 &
        export DISPLAY=:99
        for i in $(seq 50); do [ -e /tmp/.X11-unix/X99 ] && break; sleep 0.1; done
        dbus-run-session -- node /spike/linux/probe.mjs run --app /app/cc-switch --runs '"$RUNS"' --out /out/results.json'
    ;;
  *)
    echo "usage: $0 build|probe" >&2
    exit 2
    ;;
esac
