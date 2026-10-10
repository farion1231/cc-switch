import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ColumnVisibilityMenu } from "@/components/ui/ColumnVisibilityMenu";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) =>
      (
        ({
          "common.columnVisibility.label": "Columns",
          "common.columnVisibility.reset": "Reset columns",
          "common.columnVisibility.required": "Required",
        }) as Record<string, string>
      )[key] ?? key,
  }),
}));

const columns = [
  { id: "time", label: "Time", required: true },
  { id: "model", label: "Model" },
  { id: "firstToken", label: "First token" },
] as const;

describe("ColumnVisibilityMenu", () => {
  it("shows controlled checkboxes and keeps the menu open after a choice", async () => {
    const user = userEvent.setup();
    const onVisibleChange = vi.fn();
    const onReset = vi.fn();
    const { rerender } = render(
      <ColumnVisibilityMenu
        columns={columns}
        visibility={{ firstToken: false }}
        onVisibleChange={onVisibleChange}
        onReset={onReset}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Columns" }));
    expect(
      screen.getByRole("menuitemcheckbox", { name: "Model" }),
    ).toHaveAttribute("aria-checked", "true");
    const firstToken = screen.getByRole("menuitemcheckbox", {
      name: "First token",
    });
    expect(firstToken).toHaveAttribute("aria-checked", "false");
    await user.click(firstToken);
    expect(onVisibleChange).toHaveBeenCalledWith("firstToken", true);
    expect(screen.getByRole("menu")).toBeInTheDocument();

    rerender(
      <ColumnVisibilityMenu
        columns={columns}
        visibility={{ firstToken: true }}
        onVisibleChange={onVisibleChange}
        onReset={onReset}
      />,
    );
    expect(firstToken).toHaveAttribute("aria-checked", "true");
  });

  it("keeps required columns checked and disabled even with a hidden prop", async () => {
    const user = userEvent.setup();
    const onVisibleChange = vi.fn();
    render(
      <ColumnVisibilityMenu
        columns={columns}
        visibility={{ time: false }}
        onVisibleChange={onVisibleChange}
        onReset={vi.fn()}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Columns" }));
    const required = screen.getByRole("menuitemcheckbox", {
      name: "Time Required",
    });
    expect(required).toHaveAttribute("aria-checked", "true");
    expect(required).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(required);
    expect(onVisibleChange).not.toHaveBeenCalled();
  });

  it("resets the controlled choices without closing the menu", async () => {
    const user = userEvent.setup();
    const onReset = vi.fn();
    render(
      <ColumnVisibilityMenu
        columns={columns}
        visibility={{ model: false, firstToken: false }}
        onVisibleChange={vi.fn()}
        onReset={onReset}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Columns" }));
    await user.click(screen.getByRole("menuitem", { name: "Reset columns" }));
    expect(onReset).toHaveBeenCalledOnce();
    expect(screen.getByRole("menu")).toBeInTheDocument();
  });

  it("supports keyboard toggling and returns focus to the compact trigger on Escape", async () => {
    const user = userEvent.setup();
    const onVisibleChange = vi.fn();
    render(
      <ColumnVisibilityMenu
        columns={columns}
        visibility={{ firstToken: false }}
        onVisibleChange={onVisibleChange}
        onReset={vi.fn()}
        compact
      />,
    );

    const trigger = screen.getByRole("button", { name: "Columns" });
    expect(trigger).toHaveAttribute("title", "Columns");
    expect(trigger).not.toHaveTextContent("Columns");
    trigger.focus();
    await user.keyboard("{Enter}");
    await waitFor(() =>
      expect(
        screen.getByRole("menuitemcheckbox", { name: "Model" }),
      ).toHaveFocus(),
    );
    await user.keyboard("{ArrowDown}");
    expect(
      screen.getByRole("menuitemcheckbox", { name: "First token" }),
    ).toHaveFocus();
    await user.keyboard(" ");
    expect(onVisibleChange).toHaveBeenCalledWith("firstToken", true);
    expect(screen.getByRole("menu")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(trigger).toHaveFocus());
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });
});
