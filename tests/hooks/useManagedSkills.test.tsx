import type { PropsWithChildren } from "react";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { createInstance } from "i18next";
import { useInstalledSkills, useSkillBackups } from "@/hooks/useSkills";
import type { InstalledSkill } from "@/lib/api/skills";
import { formatSkillError } from "@/lib/errors/skillErrorParser";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import ja from "@/i18n/locales/ja.json";

const getInstalled = vi.hoisted(() => vi.fn());
vi.mock("@/lib/api/skills", () => ({ skillsApi: { getInstalled } }));

const manual: InstalledSkill = {
  id: "local:manual",
  name: "cc-switch-vps",
  directory: "manual",
  apps: {
    claude: true,
    codex: false,
    gemini: false,
    opencode: false,
    openclaw: false,
    hermes: false,
    pi: false,
  },
  installedAt: 1,
  updatedAt: 0,
};
const managed = {
  ...manual,
  id: "internal:vps",
  directory: "cc-switch-vps",
  managedBy: "vps",
};

function wrapper(client: QueryClient) {
  return function Wrapper({ children }: PropsWithChildren) {
    return (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
  };
}

describe("VPS-managed Skill visibility", () => {
  it("excludes managed rows from installed lists and counts without hiding a same-name manual Skill", async () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    getInstalled.mockResolvedValueOnce([manual, managed]);
    const { result, unmount } = renderHook(() => useInstalledSkills(), {
      wrapper: wrapper(client),
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toEqual([manual]);
    unmount();
    client.clear();
  });

  it("also filters cached installed rows and backup entries", () => {
    const client = new QueryClient();
    client.setQueryData(["skills", "installed"], [manual, managed]);
    const backup = (skill: InstalledSkill) => ({
      backupId: skill.id,
      backupPath: "/test",
      createdAt: 1,
      skill,
    });
    client.setQueryData(
      ["skills", "backups"],
      [backup(manual), backup(managed)],
    );
    const { result, unmount } = renderHook(
      () => ({ installed: useInstalledSkills(), backups: useSkillBackups() }),
      { wrapper: wrapper(client) },
    );
    expect(result.current.installed.data).toEqual([manual]);
    expect(result.current.backups.data).toEqual([backup(manual)]);
    unmount();
    client.clear();
  });

  it.each(Object.entries({ en, zh, "zh-TW": zhTW, ja }))(
    "explains ownership conflicts in %s",
    async (language, locale) => {
      const i18n = createInstance();
      await i18n.init({
        lng: language,
        resources: { [language]: { translation: locale } },
      });
      const result = formatSkillError(
        JSON.stringify({
          code: "SKILL_MANAGED_BY_VPS",
          context: { directory: "cc-switch-vps" },
          suggestion: "manageInVps",
        }),
        i18n.t,
      );
      expect(result.description).toContain("cc-switch-vps");
      expect(result.description).toContain("VPS");
      expect(result.description).not.toContain("skills.error.");
      expect(result.description).not.toContain("manageInVps");
    },
  );
});
