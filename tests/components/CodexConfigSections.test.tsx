import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CodexConfigSection } from "@/components/providers/forms/CodexConfigSections";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) =>
      ({
        "codexConfig.contextWindow": "上下文大小",
        "codexConfig.autoCompactLimit": "自动压缩阈值",
        "codexConfig.contextWindowGlobalHint":
          "全局上下文大小：填写后所有模型统一使用该长度；不填则以每个模型填写的上下文为准。压缩比例默认 0.95。",
      })[key] ?? key,
  }),
}));

vi.mock("@/components/JsonEditor", () => ({
  default: () => null,
}));

function renderSection(value = "", onChange = vi.fn()) {
  render(
    <CodexConfigSection
      value={value}
      onChange={onChange}
      useCommonConfig={false}
      onCommonConfigToggle={vi.fn()}
      onEditCommonConfig={vi.fn()}
    />,
  );
  return onChange;
}

describe("CodexConfigSection top-level integer fields", () => {
  it.each([
    {
      label: "上下文大小",
      field: "model_context_window",
    },
    {
      label: "自动压缩阈值",
      field: "model_auto_compact_token_limit",
    },
  ])("填写$label失焦后写入$field", ({ label, field }) => {
    const onChange = renderSection();

    const input = screen.getByLabelText(label) as HTMLInputElement;
    fireEvent.change(input, { target: { value: "200000" } });
    expect(onChange).not.toHaveBeenCalled();

    fireEvent.blur(input);
    expect(onChange).toHaveBeenCalledWith(
      expect.stringContaining(field + " = 200000"),
    );
  });

  it.each([
    {
      label: "上下文大小",
    },
    {
      label: "自动压缩阈值",
    },
  ])("$label输入框只接受数字", ({ label }) => {
    renderSection();

    const input = screen.getByLabelText(label) as HTMLInputElement;
    fireEvent.change(input, { target: { value: "12abc" } });
    // 输入过程中原样显示，失焦解析失败后回退到已存值（此处为空）。
    expect(input).toHaveValue("12abc");

    fireEvent.blur(input);
    expect(input).toHaveValue("");
  });

  it.each([
    {
      label: "上下文大小",
      field: "model_context_window",
    },
    {
      label: "自动压缩阈值",
      field: "model_auto_compact_token_limit",
    },
  ])("清空$label失焦后移除$field", ({ label, field }) => {
    const onChange = renderSection(field + " = 200000\n");

    const input = screen.getByLabelText(label) as HTMLInputElement;
    fireEvent.change(input, { target: { value: "" } });
    fireEvent.blur(input);

    const updated = onChange.mock.calls[0][0] as string;
    expect(updated).not.toMatch(new RegExp("^" + field + "\\s*=", "m"));
  });

  it("输入 500k 失焦后写入 model_context_window = 500000 且原样显示", () => {
    const onChange = renderSection();

    const input = screen.getByLabelText("上下文大小") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "500k" } });
    fireEvent.blur(input);

    expect(onChange).toHaveBeenCalledWith(
      expect.stringContaining("model_context_window = 500000"),
    );
    expect(input).toHaveValue("500k");
  });

  it("输入 2m 失焦后写入 model_auto_compact_token_limit = 2000000", () => {
    const onChange = renderSection();

    const input = screen.getByLabelText("自动压缩阈值") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "2m" } });
    fireEvent.blur(input);

    expect(onChange).toHaveBeenCalledWith(
      expect.stringContaining("model_auto_compact_token_limit = 2000000"),
    );
    expect(input).toHaveValue("2m");
  });

  it("输入 500k 后按回车只结束编辑状态并写入解析值", () => {
    const onChange = renderSection();

    const input = screen.getByLabelText("上下文大小") as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: "500k" } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(onChange).toHaveBeenCalledWith(
      expect.stringContaining("model_context_window = 500000"),
    );
    expect(input).toHaveValue("500k");
  });

  it("展示全局上下文大小说明", () => {
    renderSection();

    expect(
      screen.getByText(
        "全局上下文大小：填写后所有模型统一使用该长度；不填则以每个模型填写的上下文为准。压缩比例默认 0.95。",
      ),
    ).toBeInTheDocument();
  });
});
