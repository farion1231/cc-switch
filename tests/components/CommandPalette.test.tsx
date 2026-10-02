import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { CommandPalette } from "@/components/shell/CommandPalette";

// cmdk 依赖 ResizeObserver / scrollIntoView，jsdom 里没有
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
} as unknown as typeof ResizeObserver;
Element.prototype.scrollIntoView ??= () => undefined;

function renderPalette() {
  const props = {
    open: true,
    onOpenChange: vi.fn(),
    visibleApps: { opencode: false } as never,
    onSelectApp: vi.fn(),
    onSelectPage: vi.fn(),
    onOpenSettings: vi.fn(),
  };
  render(<CommandPalette {...props} />);
  return props;
}

describe("CommandPalette", () => {
  it("jumps to an app by name and closes", async () => {
    const user = userEvent.setup();
    const props = renderPalette();

    await user.type(screen.getByRole("combobox"), "codex");
    await user.keyboard("{Enter}");

    expect(props.onSelectApp).toHaveBeenCalledWith("codex");
    expect(props.onOpenChange).toHaveBeenCalledWith(false);
  });

  it("lists pages and settings, and leaves out hidden apps", async () => {
    const user = userEvent.setup();
    const props = renderPalette();

    expect(screen.queryByText("OpenCode")).not.toBeInTheDocument();
    await user.click(screen.getByText("MCP"));
    expect(props.onSelectPage).toHaveBeenCalledWith("mcp");
  });
});
