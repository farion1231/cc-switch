import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import {
  buildClaudeRouterBaseUrl,
  buildClaudeRouterVsCodeSettings,
  ClaudeRouterSetupDialog,
} from "@/components/proxy/ClaudeRouterSetupDialog";

describe("ClaudeRouterSetupDialog", () => {
  it("uses the configured port and copies the exact loopback-only VS Code settings", async () => {
    const user = userEvent.setup();
    const clipboardWrite = vi
      .spyOn(navigator.clipboard, "writeText")
      .mockResolvedValue();
    render(
      <ClaudeRouterSetupDialog
        open
        onOpenChange={vi.fn()}
        listenAddress="127.0.0.1"
        port={18888}
        exposedModelCount={3}
      />,
    );

    expect(
      screen.getByText("http://127.0.0.1:18888/claude-router"),
    ).toBeInTheDocument();
    expect(screen.getByText("3 models exposed")).toBeInTheDocument();
    expect(
      screen.getByText(/merge these entries into the existing array/i),
    ).toBeInTheDocument();
    expect(screen.getByText(/Developer: Reload Window/i)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Copy settings" }));
    await waitFor(() => expect(clipboardWrite).toHaveBeenCalledTimes(1));

    const copied = clipboardWrite.mock.calls[0][0] as string;
    expect(JSON.parse(copied)).toEqual({
      "claudeCode.environmentVariables": [
        {
          name: "ANTHROPIC_BASE_URL",
          value: "http://127.0.0.1:18888/claude-router",
        },
        { name: "ANTHROPIC_AUTH_TOKEN", value: "PROXY_MANAGED" },
        { name: "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY", value: "1" },
      ],
      "claudeCode.disableLoginPrompt": true,
    });
    expect(copied).toBe(buildClaudeRouterVsCodeSettings(18888));
    expect(copied).not.toContain("sk-provider-secret");
    expect(document.body.textContent).not.toContain("sk-provider-secret");
  });
  it.each([
    ["127.0.0.1", "http://127.0.0.1:15721/claude-router"],
    ["localhost", "http://127.0.0.1:15721/claude-router"],
    ["0.0.0.0", "http://127.0.0.1:15721/claude-router"],
    ["192.168.1.20", "http://192.168.1.20:15721/claude-router"],
    ["::", "http://[::1]:15721/claude-router"],
    ["::1", "http://[::1]:15721/claude-router"],
    ["2001:db8::10", "http://[2001:db8::10]:15721/claude-router"],
  ])("builds a reachable client URL for %s", (listenAddress, expected) => {
    expect(buildClaudeRouterBaseUrl(listenAddress, 15721)).toBe(expected);
  });
});
