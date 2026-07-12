import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import PluginsPage from "@/components/plugins/PluginsPage";

const mutationMock = vi.hoisted(() => ({
  isPending: false,
  mutateAsync: vi.fn(),
}));
const pluginsMock = vi.hoisted(() => vi.fn());

vi.mock("@/hooks/usePlugins", () => ({
  usePluginStatuses: () => ({
    data: [
      { app: "codex", available: true, version: "codex 1" },
      { app: "claude", available: false, error: "not installed" },
    ],
  }),
  usePlugins: pluginsMock,
  usePluginMarketplaces: () => ({ data: [] }),
  usePluginMutation: () => mutationMock,
}));

vi.mock("@/lib/api", () => ({
  settingsApi: { pickDirectory: vi.fn() },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

const installedPlugin = {
  pluginId: "ponytail@ponytail",
  name: "ponytail",
  version: "4.8.4",
  app: "codex" as const,
  marketplaceName: "ponytail",
  installed: true,
  enabled: true,
  supportedActions: {
    install: false,
    update: false,
    enable: false,
    disable: true,
    uninstall: true,
  },
};

describe("PluginsPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    pluginsMock.mockImplementation((app: string, includeAvailable: boolean) => {
      if (includeAvailable) return { data: [], isLoading: false };
      if (app === "claude") {
        return {
          data: [],
          isLoading: false,
          error: new Error("claude failed"),
        };
      }
      return { data: [installedPlugin], isLoading: false };
    });
  });

  it("shows one client failure without hiding the other client", () => {
    render(<PluginsPage />);
    expect(screen.getByText("ponytail")).toBeInTheDocument();
    expect(screen.getByText(/claude failed/)).toBeInTheDocument();
  });

  it("does not uninstall when confirmation is cancelled", async () => {
    const user = userEvent.setup();
    render(<PluginsPage />);

    await user.click(screen.getByTitle("plugins.uninstall"));
    await user.click(screen.getByText("common.cancel"));

    expect(mutationMock.mutateAsync).not.toHaveBeenCalled();
  });
});
