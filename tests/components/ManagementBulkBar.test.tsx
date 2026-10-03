import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ManagementBulkBar } from "@/components/common/ManagementBulkBar";

type ManagementBulkBarProps = Parameters<typeof ManagementBulkBar>[0];

const renderBar = (overrides: Partial<ManagementBulkBarProps> = {}) => {
  const props: ManagementBulkBarProps = {
    selectedCount: 0,
    totalCount: 0,
    onClear: vi.fn(),
    selectAllLabel: "select all",
    clearLabel: "clear",
    toolbarLabel: "bulk actions",
    ...overrides,
  };
  return render(<ManagementBulkBar {...props} />);
};

describe("ManagementBulkBar", () => {
  it("renders nothing while nothing is selected or hidden", () => {
    const { container } = renderBar();

    expect(container).toBeEmptyDOMElement();
  });

  it("renders the fraction and select-all while rows are selected", () => {
    renderBar({ selectedCount: 2, totalCount: 5, onSelectAll: vi.fn() });

    expect(
      screen.getByRole("toolbar", { name: "bulk actions" }),
    ).toBeInTheDocument();
    expect(screen.getByText("2 / 5")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "select all" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "clear" })).toBeInTheDocument();
  });

  it("stays mounted while only hidden selections remain", () => {
    renderBar({
      selectedCount: 0,
      totalCount: 12,
      hiddenCount: 3,
      onSelectAll: vi.fn(),
    });

    // The fraction describes the screen (0 visible rows selected); the hidden
    // count only decides that the bar — with the tenant's disclosures — stays.
    expect(
      screen.getByRole("toolbar", { name: "bulk actions" }),
    ).toBeInTheDocument();
    expect(screen.getByText("0 / 12")).toBeInTheDocument();
    expect(screen.queryByText("3 / 12")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "clear" })).toBeInTheDocument();
  });

  it("hides select-all once every visible row is selected", () => {
    renderBar({ selectedCount: 5, totalCount: 5, onSelectAll: vi.fn() });

    expect(
      screen.queryByRole("button", { name: "select all" }),
    ).not.toBeInTheDocument();
  });

  it("offers select-all again once the selection drops below the total", () => {
    renderBar({ selectedCount: 2, totalCount: 5, onSelectAll: vi.fn() });

    expect(
      screen.getByRole("button", { name: "select all" }),
    ).toBeInTheDocument();
  });
});
