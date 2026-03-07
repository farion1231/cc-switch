#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
HARNESS_MANIFEST="$ROOT/qa/cli-e2e/Cargo.toml"

"$ROOT/qa/cli-e2e/scripts/build-cli.sh"
cargo run --manifest-path "$HARNESS_MANIFEST" -- run-all
