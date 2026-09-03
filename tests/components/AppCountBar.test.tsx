import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AppCountBar } from "@/components/common/AppCountBar";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, params?: { app?: string }) =>
      params?.app ? `${key}:${params.app}` : key,
  }),
}));

describe("AppCountBar", () => {
  it("keeps legacy counts non-interactive without a bulk callback", () => {
    render(
      <AppCountBar
        totalLabel="2 items"
        counts={{ claude: 1 }}
        appIds={["claude"]}
      />,
    );

    expect(screen.getByText("2 items")).toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
  });

  it("uses the whole badge to turn none or mixed into all and all into none", () => {
    const onToggleAll = vi.fn();
    const { rerender } = render(
      <AppCountBar
        totalLabel="2 items"
        totalCount={2}
        counts={{ claude: 1 }}
        appIds={["claude"]}
        onToggleAll={onToggleAll}
      />,
    );

    const mixed = screen.getByRole("checkbox", {
      name: "common.enableAllForApp:Claude",
    });
    expect(mixed).toHaveAttribute("aria-checked", "mixed");
    expect(mixed).toHaveAttribute("data-selection-state", "partial");
    expect(mixed.querySelectorAll("span")).toHaveLength(2);
    expect(mixed.querySelector("svg")).not.toBeInTheDocument();
    fireEvent.click(mixed);
    expect(onToggleAll).toHaveBeenCalledWith("claude", true);

    rerender(
      <AppCountBar
        totalLabel="2 items"
        totalCount={2}
        counts={{ claude: 2 }}
        appIds={["claude"]}
        onToggleAll={onToggleAll}
      />,
    );

    const all = screen.getByRole("checkbox", {
      name: "common.disableAllForApp:Claude",
    });
    expect(all).toHaveAttribute("aria-checked", "true");
    expect(all).toHaveAttribute("data-selection-state", "all");
    expect(all.querySelectorAll("span")).toHaveLength(2);
    expect(all.querySelector("svg")).not.toBeInTheDocument();
    fireEvent.click(all);
    expect(onToggleAll).toHaveBeenLastCalledWith("claude", false);
  });

  it("does not render a trailing selection box when no items are selected", () => {
    const onToggleAll = vi.fn();
    render(
      <AppCountBar
        totalLabel="2 items"
        totalCount={2}
        counts={{ claude: 0 }}
        appIds={["claude"]}
        onToggleAll={onToggleAll}
      />,
    );

    const none = screen.getByRole("checkbox", {
      name: "common.enableAllForApp:Claude",
    });
    expect(none).toHaveAttribute("aria-checked", "false");
    expect(none).toHaveAttribute("data-selection-state", "none");
    expect(none.querySelectorAll("span")).toHaveLength(2);
    expect(none.querySelector("svg")).not.toBeInTheDocument();

    fireEvent.click(none);
    expect(onToggleAll).toHaveBeenCalledWith("claude", true);
  });

  it("disables bulk controls for an empty list or while any app is pending", () => {
    const onToggleAll = vi.fn();
    const { rerender } = render(
      <AppCountBar
        totalLabel="0 items"
        totalCount={0}
        counts={{ claude: 0 }}
        appIds={["claude"]}
        onToggleAll={onToggleAll}
      />,
    );

    expect(screen.getByRole("checkbox")).toBeDisabled();

    rerender(
      <AppCountBar
        totalLabel="2 items"
        totalCount={2}
        counts={{ claude: 1, codex: 1 }}
        appIds={["claude", "codex"]}
        pendingApp="claude"
        onToggleAll={onToggleAll}
      />,
    );

    for (const control of screen.getAllByRole("checkbox")) {
      expect(control).toBeDisabled();
      expect(control.className).not.toContain("disabled:opacity-");
    }

    const pendingControl = screen.getByRole("checkbox", {
      name: "common.enableAllForApp:Claude",
    });
    expect(pendingControl).toHaveAttribute("aria-busy", "true");
    expect(pendingControl.querySelectorAll("span")).toHaveLength(2);
    expect(pendingControl.querySelector("svg")).not.toBeInTheDocument();
  });

  it("supports disabling bulk controls during another management write", () => {
    render(
      <AppCountBar
        totalLabel="2 items"
        totalCount={2}
        counts={{ claude: 1 }}
        appIds={["claude"]}
        onToggleAll={vi.fn()}
        disabled
      />,
    );

    expect(screen.getByRole("checkbox")).toBeDisabled();
  });

  it("keeps the total and app badges in the legacy inline layout", () => {
    render(
      <AppCountBar
        totalLabel="2 items"
        totalCount={2}
        counts={{ claude: 1 }}
        appIds={["claude"]}
        onToggleAll={vi.fn()}
      />,
    );

    const bar = screen.getByText("2 items").closest(".glass");
    expect(bar).toHaveClass("items-center");
    expect(bar).not.toHaveClass("flex-col");
  });

  it("hides pointer focus rings while preserving keyboard focus styling", () => {
    render(
      <AppCountBar
        totalLabel="2 items"
        totalCount={2}
        counts={{ claude: 1 }}
        appIds={["claude"]}
        onToggleAll={vi.fn()}
      />,
    );

    expect(screen.getByRole("checkbox")).toHaveClass(
      "select-none",
      "focus:ring-0",
      "focus-visible:outline-none",
      "focus-visible:ring-2",
    );
  });
});
