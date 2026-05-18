import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { SkillsBulkActionBar } from "@/components/skills/SkillsBulkActionBar";
import type { AppId } from "@/lib/api/types";

const APP_IDS: AppId[] = [
  "claude",
  "codex",
  "gemini",
  "opencode",
  "hermes",
];

function renderBar(
  overrides: Partial<Parameters<typeof SkillsBulkActionBar>[0]> = {},
) {
  const props: Parameters<typeof SkillsBulkActionBar>[0] = {
    selectedCount: 0,
    selectedHasUpdate: 0,
    appIds: APP_IDS,
    onUninstall: vi.fn(),
    onToggleApp: vi.fn(),
    onUpdateAvailable: vi.fn(),
    onCancel: vi.fn(),
    onSelectAll: vi.fn(),
    totalVisible: 10,
    isWorking: false,
    ...overrides,
  };
  const result = render(<SkillsBulkActionBar {...props} />);
  return { ...result, props };
}

describe("SkillsBulkActionBar", () => {
  it("renders the selected-count key", () => {
    renderBar({ selectedCount: 3 });
    expect(screen.getByText(/skills\.bulk\.selected/)).toBeInTheDocument();
  });

  it("shows 'select all' label when selectedCount < totalVisible", () => {
    renderBar({ selectedCount: 0, totalVisible: 10 });
    expect(screen.getByText(/skills\.bulk\.selectAll/)).toBeInTheDocument();
  });

  it("shows 'select none' label when all visible are selected", () => {
    renderBar({ selectedCount: 10, totalVisible: 10 });
    expect(screen.getByText("skills.bulk.selectNone")).toBeInTheDocument();
  });

  it("disables uninstall when nothing is selected", () => {
    renderBar({ selectedCount: 0 });
    const btn = screen.getByText("skills.bulk.uninstall").closest("button");
    expect(btn).toBeDisabled();
  });

  it("calls onUninstall when uninstall is clicked with a selection", () => {
    const { props } = renderBar({ selectedCount: 2 });
    fireEvent.click(screen.getByText("skills.bulk.uninstall"));
    expect(props.onUninstall).toHaveBeenCalledTimes(1);
  });

  it("calls onCancel when cancel is clicked", () => {
    const { props } = renderBar({ selectedCount: 0 });
    fireEvent.click(screen.getByText("skills.bulk.cancel"));
    expect(props.onCancel).toHaveBeenCalledTimes(1);
  });

  it("calls onSelectAll when select-all link is clicked", () => {
    const { props } = renderBar();
    fireEvent.click(screen.getByText(/skills\.bulk\.selectAll/));
    expect(props.onSelectAll).toHaveBeenCalledTimes(1);
  });

  it("does not render updateAvailable button when selectedHasUpdate is 0", () => {
    renderBar({ selectedCount: 2, selectedHasUpdate: 0 });
    expect(
      screen.queryByText(/skills\.bulk\.updateAvailable/),
    ).not.toBeInTheDocument();
  });

  it("renders updateAvailable button when selectedHasUpdate > 0", () => {
    renderBar({ selectedCount: 2, selectedHasUpdate: 1 });
    expect(
      screen.getByText(/skills\.bulk\.updateAvailable/),
    ).toBeInTheDocument();
  });

  it("calls onUpdateAvailable when update button is clicked", () => {
    const { props } = renderBar({ selectedCount: 2, selectedHasUpdate: 1 });
    fireEvent.click(screen.getByText(/skills\.bulk\.updateAvailable/));
    expect(props.onUpdateAvailable).toHaveBeenCalledTimes(1);
  });

  it("disables actions while isWorking", () => {
    renderBar({ selectedCount: 2, isWorking: true });
    const uninstallBtn = screen
      .getByText("skills.bulk.uninstall")
      .closest("button");
    const enableBtn = screen
      .getByText("skills.bulk.enableIn")
      .closest("button");
    expect(uninstallBtn).toBeDisabled();
    expect(enableBtn).toBeDisabled();
  });

  it("enables enable/disable buttons when selectedCount > 0", () => {
    renderBar({ selectedCount: 1, isWorking: false });
    expect(
      screen.getByText("skills.bulk.enableIn").closest("button"),
    ).not.toBeDisabled();
    expect(
      screen.getByText("skills.bulk.disableIn").closest("button"),
    ).not.toBeDisabled();
  });

  it("opens enable menu and fires onToggleApp(app, true) on selection", async () => {
    const user = userEvent.setup();
    const { props } = renderBar({ selectedCount: 2 });
    await user.click(screen.getByText("skills.bulk.enableIn"));
    // DropdownMenu content should now be visible; click the Claude menu item.
    const claudeOption = await screen.findByRole("menuitem", { name: "Claude" });
    await user.click(claudeOption);
    expect(props.onToggleApp).toHaveBeenCalledWith("claude", true);
  });

  it("opens disable menu and fires onToggleApp(app, false) on selection", async () => {
    const user = userEvent.setup();
    const { props } = renderBar({ selectedCount: 2 });
    await user.click(screen.getByText("skills.bulk.disableIn"));
    const codexOption = await screen.findByRole("menuitem", { name: "Codex" });
    await user.click(codexOption);
    expect(props.onToggleApp).toHaveBeenCalledWith("codex", false);
  });
});
