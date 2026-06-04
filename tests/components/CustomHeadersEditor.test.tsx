import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { CustomHeadersEditor } from "@/components/providers/forms/CustomHeadersEditor";

describe("CustomHeadersEditor", () => {
  it("renders empty state", () => {
    render(<CustomHeadersEditor headers={{}} onChange={vi.fn()} />);
    expect(screen.getByText("未配置自定义请求头")).toBeInTheDocument();
  });

  it("adds a header row", () => {
    const onChange = vi.fn();
    render(<CustomHeadersEditor headers={{}} onChange={onChange} />);

    fireEvent.click(screen.getByText("添加"));

    expect(onChange).toHaveBeenCalledWith({ "": "" });
  });

  it("removes a header row", () => {
    const onChange = vi.fn();
    render(
      <CustomHeadersEditor
        headers={{ "User-Agent": "Test" }}
        onChange={onChange}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "" }));

    expect(onChange).toHaveBeenCalledWith({});
  });

  it("calls onChange when key is updated", () => {
    const onChange = vi.fn();
    render(
      <CustomHeadersEditor
        headers={{ "User-Agent": "Test" }}
        onChange={onChange}
      />,
    );

    const keyInput = screen.getAllByRole("textbox")[0];
    fireEvent.change(keyInput, { target: { value: "X-Custom" } });

    expect(onChange).toHaveBeenCalledWith({ "X-Custom": "Test" });
  });

  it("calls onChange when value is updated", () => {
    const onChange = vi.fn();
    render(
      <CustomHeadersEditor
        headers={{ "User-Agent": "Test" }}
        onChange={onChange}
      />,
    );

    const valueInput = screen.getAllByRole("textbox")[1];
    fireEvent.change(valueInput, { target: { value: "Claude Code" } });

    expect(onChange).toHaveBeenCalledWith({ "User-Agent": "Claude Code" });
  });
});
