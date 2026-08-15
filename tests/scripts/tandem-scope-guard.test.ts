import { describe, expect, it } from "vitest";
import {
  inspectScopeChanges,
  mergeChangedPaths,
} from "../../scripts/check-tandem-scope.mts";
const inspect = (path: string, content: string) =>
  inspectScopeChanges([{ path, content }]);

describe("Task 7 scope guard", () => {
  it("includes tracked and untracked changed paths exactly once", () => {
    expect(
      mergeChangedPaths(
        "package.json\nsrc/tandem/api/demoTaskGateway.ts\n",
        "e2e/tandem-ledger.spec.ts\npackage.json\n",
      ),
    ).toEqual([
      "e2e/tandem-ledger.spec.ts",
      "package.json",
      "src/tandem/api/demoTaskGateway.ts",
    ]);
  });
  it.each([
    [
      "src-tauri/tauri.conf.json",
      '{"productName":"Tandem","identifier":"dev.tandem"}',
      "identity metadata",
    ],
    [
      "src/tandem/credentialStore.ts",
      "export const apiKey = readCredential();",
      "credentials",
    ],
    [
      "src/tandem/sessionScanner.ts",
      "export function scanSessions() {}",
      "session scanning",
    ],
    ["src/tandem/terminal.ts", 'export const terminal = "iTerm2";', "iTerm2"],
    [
      "src/tandem/PresetComparison.tsx",
      "export function HandoffComparison() {}",
      "Preset/Handoff/comparison",
    ],
    [
      "src/components/unrelated.tsx",
      "export const changed = true;",
      "out-of-scope path",
    ],
  ])("rejects %s as %s", (path, content, reason) => {
    expect(inspect(path, content)).toEqual(
      expect.arrayContaining([expect.stringContaining(reason)]),
    );
  });
  it("ignores exclusion prose in docs, provenance, and tests", () => {
    expect(
      inspect(
        "docs/tandem/task-7-report.md",
        "identity metadata credentials session scanning iTerm2 Preset Handoff comparison",
      ),
    ).toEqual([]);
    expect(
      inspect(
        "tests/tandem/scope.test.ts",
        "const exclusions = 'credentials iTerm2 Handoff comparison';",
      ),
    ).toEqual([]);
    expect(
      inspect(
        ".superpowers/sdd/task-7-report.md",
        "session scanning and identity metadata are excluded",
      ),
    ).toEqual([]);
    expect(
      inspect(
        "scratch/task-7-report.md",
        "exact execution evidence and environment limitations",
      ),
    ).toEqual([]);
  });
  it.each([
    "package.json.extra",
    "pnpm-lock.yaml.extra",
    "playwright.config.ts.extra",
    "vitest.config.ts.extra",
    "scripts/check-tandem-scope.mts.extra",
    "src/tandem/api/demoTaskGateway.ts.extra",
    "src/tandem/demo/DemoLegacyConfigApp.tsx.extra",
  ])("rejects an .extra variant of allowlisted file %s", (path) => {
    expect(inspect(path, "generated scope noise")).toEqual([
      path + ": out-of-scope path",
    ]);
  });

  it("does not allow generated scope declaration noise", () => {
    expect(
      inspect(
        "scripts/check-tandem-scope.d.mts",
        "export function inspectScopeChanges(): string[];",
      ),
    ).toEqual(["scripts/check-tandem-scope.d.mts: out-of-scope path"]);
  });
  it("allows the planned Task 7 harness and deterministic demo fixture", () => {
    for (const path of [
      "package.json",
      "pnpm-lock.yaml",
      "playwright.config.ts",
      "vitest.config.ts",
      "e2e/tandem-ledger.spec.ts",
      "scripts/check-tandem-scope.mts",
      "src/tandem/api/demoTaskGateway.ts",
      "src/tandem/demo/DemoLegacyConfigApp.tsx",
      "tests/tandem/TaskLedgerJourney.test.tsx",
    ]) {
      expect(inspect(path, "planned Task 7 change")).toEqual([]);
    }
  });
});
