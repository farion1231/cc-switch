import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LanAccessSection } from "@/components/settings/LanAccessSection";
import { lanWebApi, type LanWebStatus } from "@/lib/api/lanWeb";

const BASE_STATUS: LanWebStatus = {
  mode: "local",
  port: 15722,
  allowControl: false,
  remoteControlApps: [],
  installedRemoteApps: ["claude", "agy"],
  running: true,
  listenAddress: "127.0.0.1:15722",
  localUrl: "http://127.0.0.1:15722/#t=tok",
  lanUrls: [],
  lanHosts: [],
  token: "tok",
  error: null,
};

describe("LanAccessSection per-agent remote control selector", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders all 10 CLI agent toggles and updates remoteControlApps independently", async () => {
    vi.spyOn(lanWebApi, "getStatus").mockResolvedValue(BASE_STATUS);
    const updateSpy = vi.spyOn(lanWebApi, "updateConfig").mockResolvedValue({
      ...BASE_STATUS,
      remoteControlApps: ["agy"],
    });

    render(<LanAccessSection />);

    const group = await screen.findByRole("group", {
      name: /settings\.lanWeb\.remoteAgentsLabel|Agent Remote Control|Agent 远控/i,
    });
    const buttons = group.querySelectorAll("button");
    expect(buttons.length).toBe(10);

    const agyBtn = screen.getByRole("button", { name: "Antigravity" });
    expect(agyBtn).toHaveAttribute("aria-pressed", "false");

    fireEvent.click(agyBtn);

    await waitFor(() => {
      expect(updateSpy).toHaveBeenCalledWith(
        expect.objectContaining({
          mode: "local",
          port: 15722,
          allowControl: false,
          remoteControlApps: ["agy"],
        }),
      );
    });
  });

  it("shows confirmation warning when enabling the first agent in LAN mode", async () => {
    const lanStatus: LanWebStatus = {
      ...BASE_STATUS,
      mode: "lan",
      listenAddress: "0.0.0.0:15722",
      lanUrls: ["http://192.168.1.5:15722/#t=tok"],
    };
    vi.spyOn(lanWebApi, "getStatus").mockResolvedValue(lanStatus);
    const updateSpy = vi.spyOn(lanWebApi, "updateConfig").mockResolvedValue({
      ...lanStatus,
      remoteControlApps: ["claude"],
    });

    render(<LanAccessSection />);

    const claudeBtn = await screen.findByRole("button", { name: "Claude" });
    fireEvent.click(claudeBtn);

    // Must NOT call updateConfig before confirmation
    expect(updateSpy).not.toHaveBeenCalled();

    const confirmBtn = await screen.findByRole("button", {
      name: /settings\.lanWeb\.confirmRemoteButton|Enable for Claude|为 Claude 开启远控/i,
    });
    fireEvent.click(confirmBtn);

    await waitFor(() => {
      expect(updateSpy).toHaveBeenCalledWith(
        expect.objectContaining({
          mode: "lan",
          remoteControlApps: ["claude"],
        }),
      );
    });
  });
});
