import { describe, expect, it } from "vitest";
import { getToolInstallSourceLabel } from "@/components/settings/ToolInstallRow";
import {
  UNINSTALL_METHODS,
  uninstallCommandsFor,
} from "@/components/apps/toolUninstallCommands";

describe("global package sources", () => {
  it("keeps manual uninstall rows unique across platform changes", () => {
    for (const environment of ["macos", "windows", "linux"] as const) {
      for (const method of Object.keys(UNINSTALL_METHODS) as Array<
        keyof typeof UNINSTALL_METHODS
      >) {
        const rows = uninstallCommandsFor(method, () => environment);
        expect(new Set(rows.map((row) => row.command)).size).toBe(rows.length);
      }
    }
  });
  it("offers removal guidance for every CodeBuddy package manager", () => {
    for (const [method, command] of [
      ["pnpm", "pnpm remove -g @tencent-ai/codebuddy-code"],
      ["bun", "bun remove -g @tencent-ai/codebuddy-code"],
      [
        "brew-formula",
        "brew uninstall --formula Tencent-CodeBuddy/tap/codebuddy-code",
      ],
    ] as const) {
      expect(uninstallCommandsFor(method, () => "macos")).toContainEqual(
        expect.objectContaining({ tool: "codebuddy", command }),
      );
    }
  });
  it("keeps Yarn and Vite+ distinct from npm and Homebrew", () => {
    for (const [source, label] of [
      ["npm", "npm"],
      ["homebrew", "Homebrew"],
      ["yarn", "Yarn"],
      ["vite", "Vite+"],
    ]) {
      expect(getToolInstallSourceLabel(source)).toBe(label);
    }
    expect(getToolInstallSourceLabel("unknown")).toBeUndefined();
  });
  it("offers the matching documented manual removal commands", () => {
    expect(uninstallCommandsFor("yarn", () => "macos")).toEqual([
      expect.objectContaining({
        tool: "opencode",
        command: "yarn global remove @opencode/cli",
      }),
      expect.objectContaining({
        tool: "codebuddy",
        command: "yarn global remove @tencent-ai/codebuddy-code",
      }),
    ]);
    expect(uninstallCommandsFor("vite", () => "linux")).toEqual([
      expect.objectContaining({
        tool: "opencode",
        command: "vp remove -g @opencode/cli",
      }),
    ]);
    expect(uninstallCommandsFor("vite", () => "windows")).toEqual([]);
  });
});
