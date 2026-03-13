#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cd "$ROOT_DIR"

if ! command -v pnpm >/dev/null 2>&1; then
  echo "Error: pnpm is not installed or not in PATH." >&2
  exit 1
fi

if [ ! -d node_modules ]; then
  echo "Dependencies not found. Running pnpm install first..."
  pnpm install
fi

exec pnpm dev
