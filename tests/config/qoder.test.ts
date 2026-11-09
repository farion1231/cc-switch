import { afterEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_VISIBLE_APPS } from "@/config/appConfig";
import { qoderEdition, resolveVisibleApps } from "@/config/qoder";
import { installCommandsFor } from "@/components/apps/useToolManagement";
import { uninstallCommandsFor } from "@/components/apps/toolUninstallCommands";
import * as platform from "@/lib/platform";

describe("Qoder edition selection", () => {
  afterEach(() => vi.restoreAllMocks());

  it.each(["zh", "zh-CN", "zh-TW", "zh-Hant"])(
    "defaults %s to China",
    (language) => {
      expect(qoderEdition(undefined, language)).toBe("cn");
    },
  );

  it.each(["en", "ja", "tr"])("defaults %s to international", (language) => {
    expect(qoderEdition(undefined, language)).toBe("global");
  });

  it("keeps a manual choice when the language changes", () => {
    expect(qoderEdition({ qoderEdition: "global", language: "zh" })).toBe(
      "global",
    );
    expect(qoderEdition({ qoderEdition: "cn", language: "en" })).toBe("cn");
    expect(resolveVisibleApps({ language: "zh" })).toMatchObject({
      qoder: false,
      "qoder-cn": true,
    });
    expect(resolveVisibleApps({ language: "en" })).toMatchObject({
      qoder: true,
      "qoder-cn": false,
    });
  });

  it("keeps the selected edition's visibility preference", () => {
    const visible = resolveVisibleApps({
      language: "zh",
      visibleApps: { ...DEFAULT_VISIBLE_APPS, "qoder-cn": false },
    });
    expect(visible.qoder).toBe(false);
    expect(visible["qoder-cn"]).toBe(false);
  });

  it.each(["qoder", "qodercn"] as const)(
    "uses native PowerShell modules in the %s Windows install instructions",
    (tool) => {
      vi.spyOn(platform, "isWindows").mockReturnValue(true);
      const command = installCommandsFor(tool);
      const encoded = command.match(
        /# Qoder CLI(?: CN)?\n.*-EncodedCommand (\S+)/,
      )?.[1];
      expect(encoded).toBeTruthy();
      const script = Buffer.from(encoded!, "base64").toString("utf16le");
      expect(script).toContain(
        '$env:PSModulePath = "$PSHOME\\Modules;$env:PSModulePath";',
      );
      expect(script).toContain(
        tool === "qoder"
          ? "irm https://qoder.com/install.ps1 | iex"
          : "irm https://static.qoder.com.cn/qoder-cli-cn/install.ps1 | iex",
      );
    },
  );

  it.each(["qoder", "qodercn"] as const)(
    "pairs %s install and uninstall commands",
    (tool) => {
      const install = installCommandsFor(tool);
      const uninstall = uninstallCommandsFor(
        "npm",
        () => "windows",
        tool,
      ).filter(
        (command) => command.tool === "qoder" || command.tool === "qodercn",
      );
      expect(uninstall).toHaveLength(1);
      expect(uninstall[0].tool).toBe(tool);
      expect(uninstall[0].command).toContain(
        tool === "qoder" ? "@qoder-ai/qodercli" : "@qodercn-ai/qoderclicn",
      );
      // Windows instructions carry UTF-16 PowerShell; verify the decoded script too.
      const encoded = install.match(
        /# Qoder CLI(?: CN)?\n.*-EncodedCommand (\S+)/,
      )?.[1];
      const script = encoded
        ? Buffer.from(encoded, "base64").toString("utf16le")
        : install;
      expect(script).toContain(
        tool === "qoder"
          ? "https://qoder.com/install"
          : "https://static.qoder.com.cn/qoder-cli-cn/install",
      );
      expect(script).not.toContain(
        tool === "qoder" ? "qoder.com.cn" : "https://qoder.com/install",
      );
    },
  );
});
