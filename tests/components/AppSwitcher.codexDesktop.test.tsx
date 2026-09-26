import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppSwitcher } from "@/components/AppSwitcher";
import { DEFAULT_VISIBLE_APPS } from "@/config/appConfig";

describe("Codex Desktop app switcher", () => {
  beforeEach(() => localStorage.removeItem("cc-switch-last-app"));
  afterEach(() => vi.restoreAllMocks());

  it("shows Desktop after Codex with a monitor badge and remembers selection", () => {
    const onSwitch = vi.fn();
    render(<AppSwitcher activeApp="codex" onSwitch={onSwitch} />);
    const buttons = screen.getAllByRole("button");
    const cli = screen.getByRole("button", { name: "Codex" });
    const desktop = screen.getByRole("button", { name: "Codex Desktop" });
    expect(buttons.indexOf(desktop)).toBe(buttons.indexOf(cli) + 1);
    expect(desktop.querySelector(".lucide-monitor")).not.toBeNull();
    fireEvent.click(desktop);
    expect(onSwitch).toHaveBeenCalledWith("codex-desktop");
    expect(localStorage.getItem("cc-switch-last-app")).toBe("codex-desktop");
  });

  it("can hide Desktop while keeping Codex visible", () => {
    render(
      <AppSwitcher
        activeApp="codex"
        onSwitch={vi.fn()}
        visibleApps={{ ...DEFAULT_VISIBLE_APPS, "codex-desktop": false }}
      />,
    );
    expect(screen.getByRole("button", { name: "Codex" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "Codex Desktop" })).toBeNull();
  });

  it("selects Desktop from narrow-window overflow and keeps it visible when active", async () => {
    vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockReturnValue(44);
    vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(100);
    const onSwitch = vi.fn();
    const { rerender } = render(
      <AppSwitcher activeApp="codex" onSwitch={onSwitch} />,
    );
    expect(screen.queryByRole("button", { name: "Codex Desktop" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "appSwitcher.more" }));
    fireEvent.click(
      await screen.findByRole("button", { name: /Codex Desktop/ }),
    );
    expect(onSwitch).toHaveBeenCalledWith("codex-desktop");
    expect(localStorage.getItem("cc-switch-last-app")).toBe("codex-desktop");
    rerender(<AppSwitcher activeApp="codex-desktop" onSwitch={onSwitch} />);
    expect(screen.getByRole("button", { name: "Codex Desktop" })).toBeVisible();
  });
});
