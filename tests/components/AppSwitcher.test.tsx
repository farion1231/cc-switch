import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppSwitcher } from "@/components/AppSwitcher";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => (key === "apps.copilotByok" ? "VS Code Copilot" : key),
  }),
}));

class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}

describe("AppSwitcher", () => {
  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", ResizeObserverStub);
  });

  it("treats VS Code Copilot as a primary switcher item after OpenCode", () => {
    const onSwitch = vi.fn();

    render(<AppSwitcher activeApp="copilot-byok" onSwitch={onSwitch} />);

    const copilot = screen.getByRole("button", {
      name: "VS Code Copilot",
    });
    const claude = screen.getByRole("button", { name: "Claude Code" });

    expect(copilot).toHaveClass("bg-background");
    expect(claude).not.toHaveClass("bg-background");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(
      screen
        .getByRole("button", { name: "OpenCode" })
        .compareDocumentPosition(copilot) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();

    fireEvent.click(copilot);
    expect(onSwitch).not.toHaveBeenCalled();

    fireEvent.click(claude);
    expect(onSwitch).toHaveBeenCalledWith("claude");
  });

  it("does not re-select the already active regular app", () => {
    const onSwitch = vi.fn();

    render(<AppSwitcher activeApp="claude" onSwitch={onSwitch} />);

    fireEvent.click(screen.getByRole("button", { name: "Claude Code" }));
    expect(onSwitch).not.toHaveBeenCalled();
  });

  it("treats Copilot CLI as a separate primary item with the Copilot glyph", () => {
    const onSwitch = vi.fn();
    render(<AppSwitcher activeApp="copilot-cli" onSwitch={onSwitch} />);

    const vscode = screen.getByRole("button", { name: "VS Code Copilot" });
    const cli = screen.getByRole("button", { name: "Copilot CLI" });
    expect(cli).toHaveClass("bg-background");
    expect(cli.querySelector("svg")).not.toBeNull();
    expect(cli.querySelector("img")).toBeNull();
    expect(
      vscode.compareDocumentPosition(cli) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();

    fireEvent.click(vscode);
    expect(onSwitch).toHaveBeenCalledWith("copilot-byok");
  });

  it("hides VS Code Copilot when it is disabled in visible apps", () => {
    render(
      <AppSwitcher
        activeApp="claude"
        onSwitch={vi.fn()}
        visibleApps={{
          claude: true,
          "claude-desktop": true,
          codex: true,
          gemini: true,
          grokbuild: true,
          opencode: true,
          openclaw: true,
          hermes: true,
          pi: true,
          copilotByok: false,
          copilotCli: true,
        }}
      />,
    );

    expect(
      screen.queryByRole("button", { name: "VS Code Copilot" }),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Claude Code" })).toBeVisible();
  });

  it("hides Copilot CLI independently from VS Code Copilot", () => {
    render(
      <AppSwitcher
        activeApp="claude"
        onSwitch={vi.fn()}
        visibleApps={{
          claude: true,
          "claude-desktop": true,
          codex: true,
          gemini: true,
          grokbuild: true,
          opencode: true,
          openclaw: true,
          hermes: true,
          pi: true,
          copilotByok: true,
          copilotCli: false,
        }}
      />,
    );

    expect(
      screen.getByRole("button", { name: "VS Code Copilot" }),
    ).toBeVisible();
    expect(
      screen.queryByRole("button", { name: "Copilot CLI" }),
    ).not.toBeInTheDocument();
  });
});
