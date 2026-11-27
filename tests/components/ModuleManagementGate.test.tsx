import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  render,
  renderHook,
  screen,
  waitFor,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Server } from "lucide-react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  ModuleManagementHeaderSwitch,
  ModuleManagementOffPage,
  useModuleManagement,
  type ManagedModule,
} from "@/components/common/ModuleManagementGate";
import { settingsApi } from "@/lib/api";
import { toast } from "@/lib/toast";
import { setSettings } from "../msw/state";
import { createElement, type ReactNode } from "react";

function TestHarness({
  module,
  switchDisabled = false,
}: {
  module: ManagedModule;
  switchDisabled?: boolean;
}) {
  const { enabled, isPending, setEnabled } = useModuleManagement(module);

  if (!enabled) {
    return (
      <ModuleManagementOffPage
        module={module}
        icon={<Server className="h-5 w-5" />}
        title={module.toUpperCase()}
        pending={isPending}
        onEnable={() => void setEnabled(true)}
      />
    );
  }

  return (
    <div data-testid={`${module}-enabled`}>
      <ModuleManagementHeaderSwitch
        module={module}
        disabled={switchDisabled || isPending}
        onDisable={() => void setEnabled(false)}
      />
    </div>
  );
}

const renderHarness = (module: ManagedModule, switchDisabled = false) =>
  render(
    <QueryClientProvider
      client={
        new QueryClient({
          defaultOptions: { queries: { retry: false } },
        })
      }
    >
      <TestHarness module={module} switchDisabled={switchDisabled} />
    </QueryClientProvider>,
  );

describe("ModuleManagementGate", () => {
  beforeEach(() => {
    setSettings({ manageMcp: false, manageSkills: false });
  });

  const hookWrapper = () => {
    const client = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    return ({ children }: { children: ReactNode }) =>
      createElement(QueryClientProvider, { client }, children);
  };

  it("preserves both choices when module settings saves overlap", async () => {
    const { result } = renderHook(
      () => ({
        mcp: useModuleManagement("mcp"),
        skills: useModuleManagement("skills"),
      }),
      { wrapper: hookWrapper() },
    );
    let releaseSave!: () => void;
    const heldSave = new Promise<void>((resolve) => {
      releaseSave = resolve;
    });
    const originalSave = settingsApi.setModuleManagement;
    const saveSpy = vi
      .spyOn(settingsApi, "setModuleManagement")
      .mockImplementationOnce(async (module, enabled) => {
        await heldSave;
        return originalSave(module, enabled);
      });
    try {
      await act(async () => {
        const first = result.current.mcp.setEnabled(true);
        await waitFor(() => expect(saveSpy).toHaveBeenCalledTimes(1));
        const second = result.current.skills.setEnabled(true);
        await second;
        setSettings({ currentProviderClaude: "from-tray" });
        releaseSave();
        await first;
      });
      const saved = await settingsApi.get();
      expect(saved.manageMcp).toBe(true);
      expect(saved.manageSkills).toBe(true);
      expect(saved.currentProviderClaude).toBe("from-tray");
    } finally {
      releaseSave();
      saveSpy.mockRestore();
    }
  });

  it("does not let an earlier Undo bypass a module write in progress", async () => {
    const successSpy = vi.spyOn(toast, "success");
    const { result, rerender } = renderHook(
      ({ blocked }) => useModuleManagement("skills", blocked),
      { initialProps: { blocked: false }, wrapper: hookWrapper() },
    );
    try {
      await act(async () => {
        await result.current.setEnabled(true);
      });
      const undo = successSpy.mock.calls[0]?.[1]?.action as
        | { onClick: () => void }
        | undefined;
      expect(undo).toBeDefined();
      rerender({ blocked: true });
      await act(async () => {
        undo?.onClick();
      });
      expect((await settingsApi.get()).manageSkills).toBe(true);

      rerender({ blocked: false });
      await act(async () => {
        undo?.onClick();
      });
      await waitFor(async () => {
        expect((await settingsApi.get()).manageSkills).toBe(false);
      });
    } finally {
      successSpy.mockRestore();
    }
  });

  it("enables MCP management, emits undoable toast, and undo turns it back off", async () => {
    const user = userEvent.setup();
    const successSpy = vi.spyOn(toast, "success");
    const infoSpy = vi.spyOn(toast, "info");

    try {
      renderHarness("mcp");

      expect(
        await screen.findByTestId("mcp-management-off"),
      ).toBeInTheDocument();
      expect(
        screen.getByText("moduleManagement.mcp.offDescription"),
      ).toBeInTheDocument();

      await user.click(
        screen.getByRole("button", {
          name: "moduleManagement.mcp.enableButton",
        }),
      );

      expect(await screen.findByTestId("mcp-enabled")).toBeInTheDocument();
      expect((await settingsApi.get()).manageMcp).toBe(true);
      expect(successSpy).toHaveBeenCalledWith(
        "moduleManagement.mcp.toastEnabled",
        expect.objectContaining({
          action: expect.objectContaining({ label: "appMatrix.undo" }),
        }),
      );

      const undoAction = successSpy.mock.calls[0]?.[1]?.action as
        | { onClick: () => void }
        | undefined;
      undoAction?.onClick();

      expect(
        await screen.findByTestId("mcp-management-off"),
      ).toBeInTheDocument();
      expect((await settingsApi.get()).manageMcp).toBe(false);
      expect(infoSpy).toHaveBeenCalledWith(
        "moduleManagement.mcp.toastDisabled",
        expect.not.objectContaining({ action: expect.anything() }),
      );
    } finally {
      successSpy.mockRestore();
      infoSpy.mockRestore();
    }
  });

  it("disables Skills management from the header switch with an undoable toast", async () => {
    setSettings({ manageSkills: true });
    const user = userEvent.setup();
    const infoSpy = vi.spyOn(toast, "info");

    try {
      renderHarness("skills");

      const headerSwitch = await screen.findByRole("switch", {
        name: "moduleManagement.skills.headerSwitchAria",
      });
      await user.click(headerSwitch);

      expect(
        await screen.findByTestId("skills-management-off"),
      ).toBeInTheDocument();
      expect((await settingsApi.get()).manageSkills).toBe(false);
      expect(infoSpy).toHaveBeenCalledWith(
        "moduleManagement.skills.toastDisabled",
        expect.objectContaining({
          action: expect.objectContaining({ label: "appMatrix.undo" }),
        }),
      );

      const undoAction = infoSpy.mock.calls[0]?.[1]?.action as
        | { onClick: () => void }
        | undefined;
      undoAction?.onClick();

      expect(await screen.findByTestId("skills-enabled")).toBeInTheDocument();
      await waitFor(async () => {
        expect((await settingsApi.get()).manageSkills).toBe(true);
      });
    } finally {
      infoSpy.mockRestore();
    }
  });

  it("keeps the header switch disabled while page interactions are blocked", async () => {
    setSettings({ manageMcp: true });
    renderHarness("mcp", true);

    const headerSwitch = await screen.findByRole("switch", {
      name: "moduleManagement.mcp.headerSwitchAria",
    });
    expect(headerSwitch).toBeDisabled();
  });
});
