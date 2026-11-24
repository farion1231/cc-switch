import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
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
