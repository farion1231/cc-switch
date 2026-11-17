import { useState } from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { useForm } from "react-hook-form";
import { describe, expect, it, vi } from "vitest";

import type { ClaudeStackModel } from "@/types";
import {
  ClaudeStackModelsField,
  createClaudeStackModelRow,
  normalizeClaudeStackModels,
  parseContextWindowInput,
} from "./ClaudeStackModelsField";
import { Form } from "@/components/ui/form";

describe("normalizeClaudeStackModels: contextWindow", () => {
  it("keeps positive integers and drops invalid values", () => {
    expect(
      normalizeClaudeStackModels([
        { model: "gpt-6.1-sol", contextWindow: 800000 },
        { model: "glm-5.2", contextWindow: 0 },
        { model: "kimi-k3", contextWindow: -5 },
        { model: "kimi-k3-air", contextWindow: Number.NaN },
        { model: "m" },
      ]),
    ).toEqual([
      { model: "gpt-6.1-sol", contextWindow: 800000 },
      { model: "glm-5.2" },
      { model: "kimi-k3" },
      { model: "kimi-k3-air" },
      { model: "m" },
    ]);
  });

  it("keeps the first window of duplicated models", () => {
    expect(
      normalizeClaudeStackModels([
        { model: "kimi-k3" },
        { model: "kimi-k3", contextWindow: 96000 },
        { model: "glm-5.2", contextWindow: 200000 },
        { model: "glm-5.2", contextWindow: 800000 },
      ]),
    ).toEqual([
      { model: "kimi-k3", contextWindow: 96000 },
      { model: "glm-5.2", contextWindow: 200000 },
    ]);
  });

  it("keeps the window of 1M rows so unchecking 1M restores it", () => {
    const rows: ClaudeStackModel[] = [
      { model: "glm-5.2", oneM: true, contextWindow: 1000000 },
    ];
    expect(normalizeClaudeStackModels(rows)).toEqual([
      { model: "glm-5.2", oneM: true, contextWindow: 1000000 },
    ]);
  });
});

describe("parseContextWindowInput", () => {
  it("parses complete numeric input by value, not by stripping characters", () => {
    expect(parseContextWindowInput("8e5")).toBe(800000);
    expect(parseContextWindowInput("2.72e5")).toBe(272000);
    expect(parseContextWindowInput("800000.0")).toBe(800000);
    expect(parseContextWindowInput("800000")).toBe(800000);
  });

  it("clears empty, malformed, fractional and out-of-range input", () => {
    expect(parseContextWindowInput("")).toBeUndefined();
    expect(parseContextWindowInput("12abc")).toBeUndefined();
    expect(parseContextWindowInput("0")).toBeUndefined();
    expect(parseContextWindowInput("-128000")).toBeUndefined();
    expect(parseContextWindowInput("1.5")).toBeUndefined();
    // 超出安全整数：JSON.stringify(1e21) 会写成指数记法，后端 u64 解不了
    expect(parseContextWindowInput("1e21")).toBeUndefined();
  });
});

describe("ClaudeStackModelsField: contextWindow input", () => {
  /** 真实受控回路：onRowsChange 回灌 state，输入框跟着行数据重渲染。 */
  const renderField = (onRowsChange: ReturnType<typeof vi.fn>) => {
    function FieldHarness() {
      const form = useForm();
      const [rows, setRows] = useState(() => [
        createClaudeStackModelRow({ model: "glm-5.2" }),
      ]);
      return (
        <Form {...form}>
          <ClaudeStackModelsField
            rows={rows}
            onRowsChange={(next) => {
              onRowsChange(next);
              setRows(next);
            }}
            fetchedModels={[]}
            onFetchModels={() => {}}
            isFetchingModels={false}
          />
        </Form>
      );
    }
    render(<FieldHarness />);
    return screen.getByLabelText("上下文窗口");
  };

  it("keeps the numeric meaning of pasted scientific notation and trailing .0", () => {
    const onRowsChange = vi.fn();
    const input = renderField(onRowsChange);

    fireEvent.change(input, { target: { value: "8e5" } });
    expect(onRowsChange).toHaveBeenLastCalledWith([
      expect.objectContaining({ model: "glm-5.2", contextWindow: 800000 }),
    ]);

    fireEvent.change(input, { target: { value: "800000.0" } });
    expect(onRowsChange).toHaveBeenLastCalledWith([
      expect.objectContaining({ model: "glm-5.2", contextWindow: 800000 }),
    ]);

    fireEvent.change(input, { target: { value: "2.72e5" } });
    expect(onRowsChange).toHaveBeenLastCalledWith([
      expect.objectContaining({ model: "glm-5.2", contextWindow: 272000 }),
    ]);
  });

  it("rejects negative values instead of silently dropping the sign", () => {
    const onRowsChange = vi.fn();
    const input = renderField(onRowsChange);

    fireEvent.change(input, { target: { value: "-128000" } });
    expect(onRowsChange).toHaveBeenLastCalledWith([
      expect.objectContaining({ model: "glm-5.2", contextWindow: undefined }),
    ]);
  });

  it("clears the window on empty input", () => {
    const onRowsChange = vi.fn();
    const input = renderField(onRowsChange);

    fireEvent.change(input, { target: { value: "200000" } });
    fireEvent.change(input, { target: { value: "" } });
    expect(onRowsChange).toHaveBeenLastCalledWith([
      expect.objectContaining({ model: "glm-5.2", contextWindow: undefined }),
    ]);
  });
});
