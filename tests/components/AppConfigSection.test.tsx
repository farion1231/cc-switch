import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { expect, it, vi } from "vitest";
import { AppConfigSection } from "@/components/settings/sections/AppConfigSection";
import type {
  SettingsFormState,
  ResolvedDirectories,
} from "@/hooks/useSettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));
const config = {
  primary: "windows" as const,
  windows: { enabled: true, dir: "C:/before" },
  wsl: [
    { distro: "Ubuntu", enabled: true, dir: "//wsl$/Ubuntu/home/me/.claude" },
  ],
};
vi.mock("@/hooks/useAppTargets", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/hooks/useAppTargets")>()),
  useAppTargetsOverview: () => ({
    data: {
      multiEnv: false,
      inventory: {
        hostWindows: true,
        wslAvailable: true,
        distros: [
          { distro: "Ubuntu", available: true, running: false, cached: true },
        ],
      },
      installs: {},
      apps: [
        {
          app: "claude",
          source: "saved",
          config,
          resolvedDirs: {},
          records: {},
        },
      ],
    },
  }),
}));
const saveMock = vi.fn();
vi.mock("@/lib/api/appTargets", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/api/appTargets")>()),
  appTargetsApi: {
    inspect: vi.fn(async () => ({
      errors: [],
      conflicts: [],
      resolvedDirs: {},
      primaryTarget: "windows",
    })),
    save: (...args: unknown[]) => saveMock(...args),
  },
}));
vi.mock("@/components/settings/CodexAuthSettings", () => ({
  CodexAuthSettings: () => null,
}));

it("saves the Windows directory through app targets while all WSL targets are stopped", async () => {
  saveMock.mockResolvedValue({
    saved: true,
    errors: [],
    conflicts: [],
    targets: [],
    imported: [],
  });
  const legacySave = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient()}>
      <AppConfigSection
        settings={{ claudeConfigDir: "C:/before" } as SettingsFormState}
        savedSettings={{
          claudeConfigDir: "C:/before",
          showInTray: true,
          minimizeToTrayOnClose: true,
        }}
        resolvedDirs={{} as ResolvedDirectories}
        isSaving={false}
        onAutoSave={vi.fn()}
        onDirectoryChange={vi.fn()}
        onBrowseDirectory={vi.fn()}
        onResetDirectory={vi.fn()}
        onSaveDirectories={legacySave}
      />
    </QueryClientProvider>,
  );
  expect(screen.queryByText("WSL · Ubuntu")).not.toBeInTheDocument();
  fireEvent.change(screen.getByDisplayValue("C:/before"), {
    target: { value: "C:/after" },
  });
  fireEvent.click(screen.getByRole("button", { name: "common.save" }));
  await waitFor(() =>
    expect(saveMock).toHaveBeenCalledWith(
      "claude",
      {
        ...config,
        windows: { ...config.windows, dir: "C:/after" },
      },
      {},
    ),
  );
  expect(legacySave).not.toHaveBeenCalled();
});
