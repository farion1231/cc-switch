import { useState } from "react";
import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Settings } from "@/types";
import { CopilotVscodeSettings } from "@/components/settings/CopilotVscodeSettings";
import {
  copilotVscodeApi,
  type CopilotVscodeTarget,
} from "@/lib/api/copilotVscode";
import { toast } from "@/lib/toast";
import { renderWithQueryClient } from "../utils/testQueryClient";
import en from "@/i18n/locales/en.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";
import ja from "@/i18n/locales/ja.json";
import ko from "@/i18n/locales/ko.json";
import tr from "@/i18n/locales/tr.json";
import ru from "@/i18n/locales/ru.json";

vi.mock("@/lib/api/copilotVscode", () => ({
  copilotVscodeApi: { targets: vi.fn() },
}));
vi.mock("@/lib/toast", () => ({ toast: { error: vi.fn() } }));
const path = "/profiles/stable/chatLanguageModels.json";
const profiles: CopilotVscodeTarget[] = [
  {
    id: "stable:default",
    editionName: "Visual Studio Code",
    profileName: "Default",
    isDefault: true,
    resources: { languageModelsPath: path },
  },
  {
    id: "stable:profile:shared",
    editionName: "Visual Studio Code",
    profileName: "Shared",
    isDefault: false,
    resources: { languageModelsPath: path },
  },
];
const save = vi.fn();

function Harness({
  initial = { enabled: false, targets: [] },
  isSaving = false,
}: {
  initial?: NonNullable<Settings["copilotVscode"]>;
  isSaving?: boolean;
}) {
  const [settings, setSettings] = useState<Settings>({
    showInTray: true,
    minimizeToTrayOnClose: true,
    copilotVscode: initial,
  });
  return (
    <CopilotVscodeSettings
      settings={settings}
      isSaving={isSaving}
      onAutoSave={async (update) => {
        const result = await save(update);
        if (result) setSettings((current) => ({ ...current, ...update }));
        return result;
      }}
    />
  );
}

describe("Copilot CLI model sync settings", () => {
  beforeEach(() => {
    save.mockReset().mockResolvedValue(true);
    vi.mocked(copilotVscodeApi.targets).mockReset().mockResolvedValue(profiles);
  });

  it("uses the same single switch and help control as Claude Code integration", () => {
    renderWithQueryClient(<Harness />);
    expect(screen.getAllByRole("switch")).toHaveLength(1);
    expect(
      screen.getByRole("button", { name: "settings.copilotVscode.enable" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).toBeNull();
    expect(screen.queryByRole("textbox")).toBeNull();
    expect(screen.queryByText(path)).toBeNull();
    expect(screen.queryByRole("button", { name: "common.refresh" })).toBeNull();
    expect(copilotVscodeApi.targets).not.toHaveBeenCalled();
  });

  it("discovers every target and deduplicates inherited profiles when enabled", async () => {
    const insidersPath = "/profiles/insiders/chatLanguageModels.json";
    vi.mocked(copilotVscodeApi.targets).mockResolvedValue([
      ...profiles,
      {
        ...profiles[0],
        id: "insiders:default",
        editionName: "Visual Studio Code Insiders",
        resources: { languageModelsPath: insidersPath },
      },
    ]);
    renderWithQueryClient(<Harness />);
    const toggle = screen.getByRole("switch");
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).toBeChecked());
    expect(save).toHaveBeenCalledWith({
      copilotVscode: { enabled: true, targets: [path, insidersPath] },
    });
  });

  it("replaces removed profiles with the currently discovered targets", async () => {
    const removedPath = "/removed-profile/chatLanguageModels.json";
    renderWithQueryClient(
      <Harness initial={{ enabled: false, targets: [removedPath] }} />,
    );
    fireEvent.click(screen.getByRole("switch"));
    await waitFor(() =>
      expect(save).toHaveBeenCalledWith({
        copilotVscode: { enabled: true, targets: [path] },
      }),
    );
  });

  it("can turn off integration without discovering profiles again", async () => {
    vi.mocked(copilotVscodeApi.targets).mockRejectedValue(
      new Error("discovery failed"),
    );
    renderWithQueryClient(
      <Harness initial={{ enabled: true, targets: [path] }} />,
    );
    const toggle = screen.getByRole("switch");
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).not.toBeChecked());
    expect(save).toHaveBeenCalledWith({
      copilotVscode: { enabled: false, targets: [path] },
    });
    expect(copilotVscodeApi.targets).not.toHaveBeenCalled();
  });

  it("retains the previous setting when the backend rejects synchronization", async () => {
    save.mockResolvedValue(false);
    renderWithQueryClient(<Harness />);
    fireEvent.click(screen.getByRole("switch"));
    await waitFor(() => expect(save).toHaveBeenCalled());
    expect(screen.getByRole("switch")).not.toBeChecked();
  });

  it.each([
    { savedTargets: [] },
    { savedTargets: ["/removed-profile/chatLanguageModels.json"] },
  ])(
    "explains when no current profiles remain with saved targets $savedTargets",
    async ({ savedTargets }) => {
      vi.mocked(copilotVscodeApi.targets).mockResolvedValue([]);
      renderWithQueryClient(
        <Harness initial={{ enabled: false, targets: savedTargets }} />,
      );
      fireEvent.click(screen.getByRole("switch"));
      await waitFor(() =>
        expect(toast.error).toHaveBeenCalledWith(
          "settings.copilotVscode.noTargets",
        ),
      );
      expect(save).not.toHaveBeenCalled();
      expect(screen.getByRole("switch")).not.toBeChecked();
    },
  );

  it("reports discovery errors and allows retrying the switch", async () => {
    vi.mocked(copilotVscodeApi.targets).mockRejectedValueOnce(
      new Error("discovery failed"),
    );
    renderWithQueryClient(<Harness />);
    const toggle = screen.getByRole("switch");
    fireEvent.click(toggle);
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(
        "settings.copilotVscode.failed",
        { description: "discovery failed" },
      ),
    );
    await waitFor(() => expect(toggle).toBeEnabled());
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).toBeChecked());
    expect(save).toHaveBeenCalledTimes(1);
  });

  it("disables the switch while discovery or saving is in progress", async () => {
    let resolveTargets!: (value: CopilotVscodeTarget[]) => void;
    vi.mocked(copilotVscodeApi.targets).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveTargets = resolve;
      }),
    );
    const { rerender } = renderWithQueryClient(<Harness />);
    const toggle = screen.getByRole("switch");
    fireEvent.click(toggle);
    expect(toggle).toBeDisabled();
    fireEvent.click(toggle);
    expect(copilotVscodeApi.targets).toHaveBeenCalledTimes(1);
    await act(async () => resolveTargets(profiles));
    await waitFor(() => expect(toggle).toBeChecked());
    await waitFor(() => expect(toggle).toBeEnabled());
    rerender(<Harness isSaving />);
    expect(toggle).toBeDisabled();
  });

  it("includes every setting label in all seven locales", () => {
    const keys = Object.keys(en.settings.copilotVscode).sort();
    for (const locale of [zh, zhTW, ja, ko, tr, ru]) {
      expect(Object.keys(locale.settings.copilotVscode).sort()).toEqual(keys);
      expect(
        Object.values(locale.settings.copilotVscode).every(
          (value) => value.trim().length > 0,
        ),
      ).toBe(true);
    }
  });
});
