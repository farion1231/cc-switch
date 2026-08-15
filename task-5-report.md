# Task 5 Report

## Status

Task 5 implements the Tandem action-ledger application shell without adding Task 6 or Task 7 behavior.

## Requirement audit

- Production `createTaskGateway()` always returns the Tauri gateway and has no demo environment branch.
- The demo is isolated in `src/tandem/demo/main.tsx`; it injects `demoTaskGateway` and `DemoLegacyConfigApp` and imports neither production `src/main.tsx` nor `src/App.tsx`.
- The plan named `tandem-demo.html` at repository root, but this repository configures Vite with `root: "src"`. The entry is therefore adapted to `src/tandem-demo.html`, which is reachable at `/tandem-demo.html`, loads `./tandem/demo/main.tsx`, and is registered as a separate Vite build input.
- The ledger uses the exact query key `["tandem", "task-ledger"]`; successful mutations update only that cache, and route changes preserve the successful ledger query.
- Client validation trims and requires all four fields, counts Unicode scalar values for the 120-character title and 20,000-character instruction limits, and matches the Rust structured-credential rules without returning matched content. User-facing mutation errors are fixed redacted messages, and ledger rows never render original instructions.
- The shell includes the four ordered, always-visible sections, compact bordered rows, responsive navigation and dialogs, primary acceptance completion, compact completion menus elsewhere, and explicit completion confirmation.
- `tests/setupTests.ts` is unchanged because its existing `resetProviderState()` call already resets the newly added deterministic Task fixtures.

## Verification evidence

- Original Task 5 RED output is unavailable: the earlier RED run timed out and its output was not retained. This report does not claim that evidence.
- A focused credential-parity regression test was observed RED in this completion session because `@/tandem/taskValidation` did not yet exist, then GREEN after implementing the shared detector.
- `pnpm vitest run tests/tandem`: 4 files passed, 17 tests passed.
- `pnpm exec tsc --noEmit`: exit 0.
- `pnpm run build:renderer`: exit 0 and emitted both `dist/index.html` and `dist/tandem-demo.html`.
- Prettier check on the explicit Task 5 TS/TSX/HTML file set: passed after formatting only those files.
- `git diff --check`: exit 0.
- A temporary Vite server using the repository configuration returned `src/tandem-demo.html` successfully from `http://127.0.0.1:4175/tandem-demo.html`; the server was then stopped.

## Known warnings

Vitest/build output includes existing environment and dependency-age warnings (localStorage experimental warning, baseline browser mapping/caniuse age, existing mixed dynamic/static import, and large chunk warning). They did not fail Task 5 tests, type checking, or the renderer build.
