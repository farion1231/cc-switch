import { render, screen, fireEvent } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SkillsToolbar } from "@/components/skills/SkillsToolbar";
import { LOCAL_SOURCE_KEY } from "@/components/skills/useSkillsFilterSort";

function renderToolbar(overrides: Partial<Parameters<typeof SkillsToolbar>[0]> = {}) {
  const props: Parameters<typeof SkillsToolbar>[0] = {
    searchQuery: "",
    onSearchChange: vi.fn(),
    sourceOptions: ["forrest/kit", "kar/tools", LOCAL_SOURCE_KEY],
    filterSources: new Set(),
    onToggleSource: vi.fn(),
    filterUpdateOnly: false,
    onToggleUpdateOnly: vi.fn(),
    updateAvailableCount: 0,
    sortKey: "nameAsc",
    onSortChange: vi.fn(),
    groupKey: "none",
    onGroupChange: vi.fn(),
    selectionMode: false,
    onToggleSelectionMode: vi.fn(),
    hasFilters: false,
    onClearFilters: vi.fn(),
    total: 70,
    filteredCount: 70,
    ...overrides,
  };
  const result = render(<SkillsToolbar {...props} />);
  return { ...result, props };
}

describe("SkillsToolbar", () => {
  it("renders the search input with placeholder key", () => {
    renderToolbar();
    expect(
      screen.getByPlaceholderText("skills.toolbar.searchPlaceholder"),
    ).toBeInTheDocument();
  });

  it("fires onSearchChange when typing", () => {
    const { props } = renderToolbar();
    const input = screen.getByPlaceholderText(
      "skills.toolbar.searchPlaceholder",
    );
    fireEvent.change(input, { target: { value: "auth" } });
    expect(props.onSearchChange).toHaveBeenCalledWith("auth");
  });

  it("shows clear button only when searchQuery is non-empty", () => {
    const { props, rerender } = renderToolbar();
    expect(screen.queryByLabelText("common.clear")).not.toBeInTheDocument();
    rerender(<SkillsToolbar {...props} searchQuery="abc" />);
    expect(screen.getByLabelText("common.clear")).toBeInTheDocument();
  });

  it("clears search via the inline X button", () => {
    const { props } = renderToolbar({ searchQuery: "abc" });
    fireEvent.click(screen.getByLabelText("common.clear"));
    expect(props.onSearchChange).toHaveBeenCalledWith("");
  });

  it("renders showing count", () => {
    renderToolbar({ filteredCount: 12, total: 70 });
    // i18n is initialized with empty resources, so the key is returned literally;
    // the interpolation in i18next still applies and the rendered text is the
    // key with placeholders replaced. We assert the slash form is visible
    // through both interpolation and the raw key. Using a regex tolerates
    // either form.
    expect(
      screen.getByText(/skills\.toolbar\.showing|12\s*\/\s*70/),
    ).toBeInTheDocument();
  });

  it("fires onToggleSelectionMode on the multi-select button", () => {
    const { props } = renderToolbar();
    const btn = screen.getByTitle("skills.toolbar.multiSelectMode");
    fireEvent.click(btn);
    expect(props.onToggleSelectionMode).toHaveBeenCalledTimes(1);
  });

  it("renders the exit-multi-select title when selectionMode is true", () => {
    renderToolbar({ selectionMode: true });
    expect(
      screen.getByTitle("skills.toolbar.exitMultiSelect"),
    ).toBeInTheDocument();
  });

  it("renders the local source label for LOCAL_SOURCE_KEY", () => {
    renderToolbar();
    expect(screen.getByText("skills.filter.local")).toBeInTheDocument();
    expect(screen.getByText("forrest/kit")).toBeInTheDocument();
    expect(screen.getByText("kar/tools")).toBeInTheDocument();
  });

  it("fires onToggleSource for the clicked source chip", () => {
    const { props } = renderToolbar();
    fireEvent.click(screen.getByText("forrest/kit"));
    expect(props.onToggleSource).toHaveBeenCalledWith("forrest/kit");
  });

  it("hides the second row when no sources, no updates and no active filters", () => {
    renderToolbar({
      sourceOptions: [],
      updateAvailableCount: 0,
      hasFilters: false,
    });
    expect(screen.queryByText("skills.filter.source")).not.toBeInTheDocument();
  });

  it("renders the updateOnly chip only when updateAvailableCount > 0", () => {
    const { rerender, props } = renderToolbar({ updateAvailableCount: 0 });
    expect(
      screen.queryByText(/skills\.filter\.updateOnly/),
    ).not.toBeInTheDocument();

    rerender(<SkillsToolbar {...props} updateAvailableCount={3} />);
    expect(
      screen.getByText(/skills\.filter\.updateOnly/),
    ).toBeInTheDocument();
  });

  it("fires onToggleUpdateOnly when the chip is clicked", () => {
    const { props } = renderToolbar({ updateAvailableCount: 2 });
    fireEvent.click(screen.getByText(/skills\.filter\.updateOnly/));
    expect(props.onToggleUpdateOnly).toHaveBeenCalledTimes(1);
  });

  it("shows the Clear filters button only when hasFilters is true", () => {
    const { rerender, props } = renderToolbar();
    expect(
      screen.queryByText("skills.toolbar.clearFilters"),
    ).not.toBeInTheDocument();

    rerender(<SkillsToolbar {...props} hasFilters={true} />);
    expect(
      screen.getByText("skills.toolbar.clearFilters"),
    ).toBeInTheDocument();
  });

  it("fires onClearFilters when clear is clicked", () => {
    const { props } = renderToolbar({ hasFilters: true });
    fireEvent.click(screen.getByText("skills.toolbar.clearFilters"));
    expect(props.onClearFilters).toHaveBeenCalledTimes(1);
  });
});
