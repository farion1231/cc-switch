#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_root"

if [[ ! -x node_modules/.bin/vitest || ! -x node_modules/.bin/tsc ]]; then
  echo "Frontend dependencies are missing. Run: pnpm install --frozen-lockfile" >&2
  exit 1
fi

cargo fmt --manifest-path src-tauri/Cargo.toml --all --check
cargo test --manifest-path src-tauri/Cargo.toml deepseek_harness --lib
cargo test --manifest-path src-tauri/Cargo.toml dsh_is_part_of_environment_checks --lib

./node_modules/.bin/tsc --noEmit
./node_modules/.bin/vitest run \
  tests/components/DshSurface.test.ts \
  tests/components/DeepSeekHarnessProviderForm.test.tsx \
  tests/components/DeepSeekHarnessFormFields.test.tsx \
  tests/hooks/useDeepSeekHarnessFormState.test.tsx \
  tests/components/ProviderList.test.tsx \
  tests/components/ProviderCardLayout.test.ts \
  tests/config/appConfig.test.tsx \
  tests/config/deepseekHarnessProviderPresets.test.ts \
  tests/hooks/useAddProviderMutation.test.tsx

if [[ "${DSH_FULL_TESTS:-0}" == "1" ]]; then
  ./node_modules/.bin/vitest run
  cargo test --manifest-path src-tauri/Cargo.toml
fi
