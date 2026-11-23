import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { AgyDesktopPanel } from "@/components/apps/AgyDesktopPanel";
import { settingsApi } from "@/lib/api";
import { createTestQueryClient } from "../utils/testQueryClient";

describe("AgyDesktopPanel", () => {
  it("renders local installation status, opens config folder, and navigates to shared features", async () => {
    const getStatusSpy = vi
      .spyOn(settingsApi, "getAgyDesktopStatus")
      .mockResolvedValue({
        supported: true,
        installed: true,
        version: "2.34.0",
        installPath:
          "C:\\Users\\me\\AppData\\Local\\Programs\\antigravity\\Antigravity.exe",
        dataDirExists: true,
      });
    vi.spyOn(settingsApi, "getConfigStatus").mockResolvedValue({
      exists: true,
      path: "C:\\Users\\me\\.gemini\\config",
    });
    const openFolderSpy = vi
      .spyOn(settingsApi, "openConfigFolder")
      .mockResolvedValue(undefined);

    const onOpenPage = vi.fn();
    const onOpenUsage = vi.fn();
    const client = createTestQueryClient();

    const { container } = render(
      <QueryClientProvider client={client}>
        <AgyDesktopPanel onOpenPage={onOpenPage} onOpenUsage={onOpenUsage} />
      </QueryClientProvider>,
    );

    expect(container.querySelector("#main-content > div")).toHaveClass(
      "mx-auto",
      "w-full",
      "max-w-3xl",
    );

    expect(await screen.findByText("v2.34.0")).toBeInTheDocument();
    expect(
      screen.getByText(
        "C:\\Users\\me\\AppData\\Local\\Programs\\antigravity\\Antigravity.exe",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText("C:\\Users\\me\\.gemini\\config"),
    ).toBeInTheDocument();

    const user = userEvent.setup();
    await user.click(
      screen.getByRole("button", { name: "agyDesktop.openConfigFolder" }),
    );
    expect(openFolderSpy).toHaveBeenCalledWith("agy-desktop");

    await user.click(
      screen.getByRole("button", { name: "agyDesktop.refreshStatus" }),
    );
    await waitFor(() => expect(getStatusSpy).toHaveBeenCalledTimes(2));

    await user.click(screen.getByRole("button", { name: /MCP/ }));
    expect(onOpenPage).toHaveBeenCalledWith("mcp");

    await user.click(screen.getByRole("button", { name: /nav\.sessions/ }));
    expect(onOpenPage).toHaveBeenCalledWith("sessions");

    await user.click(screen.getByRole("button", { name: /nav\.usage/ }));
    expect(onOpenUsage).toHaveBeenCalledTimes(1);
  });
});
