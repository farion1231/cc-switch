import { useState } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import {
  TablePagination,
  useClientPagination,
} from "@/components/usage/TablePagination";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options && "total" in options ? `${key}:${options.total}` : key,
    i18n: { resolvedLanguage: "en", language: "en" },
  }),
}));

function Harness({ count }: { count: number }) {
  const [filter, setFilter] = useState("a");
  const rows = Array.from({ length: count }, (_, i) => `row-${i + 1}`);
  const pagination = useClientPagination(rows, filter);
  return (
    <div>
      <button type="button" onClick={() => setFilter("b")}>
        change-filter
      </button>
      <ul>
        {pagination.pageRows.map((row) => (
          <li key={row}>{row}</li>
        ))}
      </ul>
      <TablePagination
        page={pagination.page}
        totalPages={pagination.totalPages}
        total={pagination.total}
        onPageChange={pagination.setPage}
      />
    </div>
  );
}

describe("useClientPagination + TablePagination", () => {
  it("pages rows 20 at a time and resets when the filter changes", async () => {
    const user = userEvent.setup();
    render(<Harness count={45} />);

    expect(screen.getByText("usage.totalRecords:45")).toBeInTheDocument();
    expect(screen.getAllByRole("listitem")).toHaveLength(20);
    expect(screen.getByText("/ 3")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "usage.prevPage" }),
    ).toBeDisabled();

    await user.click(screen.getByRole("button", { name: "usage.nextPage" }));
    expect(screen.getByText("row-21")).toBeInTheDocument();

    // 输入页码跳到最后一页：只剩 5 行
    const input = screen.getByRole("textbox", {
      name: "usage.pageInputPlaceholder",
    });
    await user.clear(input);
    await user.type(input, "3{Enter}");
    expect(screen.getAllByRole("listitem")).toHaveLength(5);
    expect(
      screen.getByRole("button", { name: "usage.nextPage" }),
    ).toBeDisabled();

    // 超出范围的页码不生效
    await user.clear(input);
    await user.type(input, "9{Enter}");
    expect(screen.getByText("row-41")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "change-filter" }));
    expect(screen.getByText("row-1")).toBeInTheDocument();
  });

  it("draws no pagination bar when everything fits on one page", () => {
    render(<Harness count={3} />);
    expect(screen.getAllByRole("listitem")).toHaveLength(3);
    expect(
      screen.queryByRole("button", { name: "usage.nextPage" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("usage.totalRecords:3")).not.toBeInTheDocument();
  });

  it("offers rows per page, even on a single page, once there are more rows than the smallest size", async () => {
    Element.prototype.scrollIntoView = vi.fn();
    const user = userEvent.setup();
    const onPageSizeChange = vi.fn();
    const props = {
      page: 0,
      onPageChange: vi.fn(),
      pageSize: 50,
      pageSizeOptions: [10, 20, 50],
      onPageSizeChange,
    };
    const { rerender, container } = render(
      <TablePagination {...props} totalPages={1} total={8} />,
    );
    // 比最小一档还少：没什么可调的
    expect(container).toBeEmptyDOMElement();

    // 只有一页，但调小以后会分页：要能找回这个下拉
    rerender(<TablePagination {...props} totalPages={1} total={30} />);
    await user.click(
      screen.getByRole("combobox", { name: "usage.pageSizeLabel" }),
    );
    // 选项依次是 10 / 20 / 50（测试里的 t 只返回键名）
    await user.click((await screen.findAllByRole("option"))[0]);
    expect(onPageSizeChange).toHaveBeenCalledWith(10);
  });

  it("clamps to the last page when rows shrink", async () => {
    const user = userEvent.setup();
    const { rerender } = render(<Harness count={45} />);
    await user.click(screen.getByRole("button", { name: "usage.nextPage" }));
    await user.click(screen.getByRole("button", { name: "usage.nextPage" }));
    expect(screen.getByText("row-41")).toBeInTheDocument();

    rerender(<Harness count={25} />);
    expect(screen.getByText("row-21")).toBeInTheDocument();
    expect(screen.getByText("/ 2")).toBeInTheDocument();
  });
});
