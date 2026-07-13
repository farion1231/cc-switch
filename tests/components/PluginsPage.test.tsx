import { render, screen, within } from "@testing-library/react";
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

const projectInstalledClaudePlugin = {
  ...installedPlugin,
  app: "claude" as const,
  scope: "project" as const,
  projectPath: "/tmp/the_old_days",
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

  it("keeps project-installed Claude plugins discoverable for another scope", async () => {
    pluginsMock.mockImplementation((app: string, includeAvailable: boolean) => {
      if (!includeAvailable || app === "codex") {
        return { data: [], isLoading: false };
      }
      return { data: [projectInstalledClaudePlugin], isLoading: false };
    });
    const user = userEvent.setup();
    render(<PluginsPage />);

    await user.click(
      screen.getByRole("tab", { name: "plugins.tabs.discover" }),
    );
    await user.type(screen.getByPlaceholderText("plugins.search"), "ponytail");

    expect(screen.getByText("ponytail")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "plugins.install" }));
    expect(screen.getByRole("dialog")).toHaveTextContent("ponytail@ponytail");
    expect(screen.getByRole("combobox")).toHaveTextContent(
      "plugins.scope.user",
    );
  });

  it("shows the selected client filter", async () => {
    const user = userEvent.setup();
    render(<PluginsPage />);

    const all = screen.getByRole("tab", { name: "common.all" });
    const claude = screen.getByRole("tab", {
      name: "plugins.apps.claude",
    });
    expect(all).toHaveAttribute("data-state", "active");

    await user.click(claude);

    expect(claude).toHaveAttribute("data-state", "active");
    expect(all).toHaveAttribute("data-state", "inactive");
  });

  it("uses the same selected state in marketplace client tabs", async () => {
    const user = userEvent.setup();
    render(<PluginsPage />);

    await user.click(
      screen.getByRole("button", { name: "plugins.marketplaces.title" }),
    );
    const dialog = within(screen.getByRole("dialog"));
    const codex = dialog.getByRole("tab", { name: "plugins.apps.codex" });
    const claude = dialog.getByRole("tab", { name: "plugins.apps.claude" });
    expect(codex).toHaveAttribute("data-state", "active");

    await user.click(claude);

    expect(claude).toHaveAttribute("data-state", "active");
    expect(codex).toHaveAttribute("data-state", "inactive");
  });
});
