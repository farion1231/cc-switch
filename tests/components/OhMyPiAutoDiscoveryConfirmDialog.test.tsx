import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { OhMyPiAutoDiscoveryConfirmDialog } from "@/components/settings/OhMyPiAutoDiscoveryConfirmDialog";
import type { OhMyPiAgentDiscoveryProvider } from "@/lib/api/ohmypi";

const providers: OhMyPiAgentDiscoveryProvider[] = [
  { id: "claude", displayName: "Claude Code" },
  { id: "claude-plugins", displayName: "Claude Code 插件市场" },
  { id: "agents", displayName: "Agent 目录 (.agent/.agents)" },
  { id: "codex", displayName: "OpenAI Codex" },
  { id: "gemini", displayName: "Gemini CLI" },
  { id: "opencode", displayName: "OpenCode" },
  { id: "cursor", displayName: "Cursor" },
  { id: "vscode", displayName: "VS Code" },
  { id: "cline", displayName: "Cline" },
  { id: "windsurf", displayName: "Windsurf" },
  { id: "github", displayName: "GitHub Copilot" },
  { id: "agent-plugins", displayName: "Agent Plugins" },
];

describe("OhMyPiAutoDiscoveryConfirmDialog", () => {
  it("renders all 12 provider display names when open", () => {
    render(
      <OhMyPiAutoDiscoveryConfirmDialog
        isOpen
        providers={providers}
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    for (const p of providers) {
      expect(screen.getByText(p.displayName)).toBeInTheDocument();
    }
  });

  it("renders cancel and confirm buttons", () => {
    render(
      <OhMyPiAutoDiscoveryConfirmDialog
        isOpen
        providers={providers}
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    expect(
      screen.getByRole("button", { name: "common.cancel" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: "ohmypi.autoDiscovery.dialog.confirmBtn",
      }),
    ).toBeInTheDocument();
  });

  it("calls onConfirm when confirm is clicked", () => {
    const onConfirm = vi.fn();
    render(
      <OhMyPiAutoDiscoveryConfirmDialog
        isOpen
        providers={providers}
        onConfirm={onConfirm}
        onCancel={vi.fn()}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", {
        name: "ohmypi.autoDiscovery.dialog.confirmBtn",
      }),
    );
    expect(onConfirm).toHaveBeenCalledOnce();
  });

  it("calls onCancel when cancel is clicked", () => {
    const onCancel = vi.fn();
    render(
      <OhMyPiAutoDiscoveryConfirmDialog
        isOpen
        providers={providers}
        onConfirm={vi.fn()}
        onCancel={onCancel}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it("disables both buttons when pending", () => {
    render(
      <OhMyPiAutoDiscoveryConfirmDialog
        isOpen
        providers={providers}
        pending
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    expect(
      screen.getByRole("button", { name: "common.cancel" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", {
        name: "ohmypi.autoDiscovery.dialog.confirmBtn",
      }),
    ).toBeDisabled();
  });

  it("does not render when closed", () => {
    render(
      <OhMyPiAutoDiscoveryConfirmDialog
        isOpen={false}
        providers={providers}
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );

    expect(screen.queryByText("Claude Code")).not.toBeInTheDocument();
  });
});
