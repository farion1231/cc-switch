import { fireEvent, screen, within } from "@testing-library/react";
import { renderWithQueryClient as render } from "../utils/testQueryClient";
import { useForm } from "react-hook-form";
import type { PropsWithChildren } from "react";
import { describe, expect, it, vi } from "vitest";
import {
  ClaudeStackModelsField,
  createClaudeStackModelRow,
} from "@/components/providers/forms/ClaudeStackModelsField";
import { Form } from "@/components/ui/form";

const FormShell = ({ children }: PropsWithChildren) => {
  const form = useForm();
  return <Form {...form}>{children}</Form>;
};

describe("ClaudeStackModelsField model picker", () => {
  it("keeps checked rows untouched, removes unchecked ones and appends new picks", () => {
    const first = createClaudeStackModelRow({
      model: "glm-5.2[1m]",
      displayName: "GLM",
    });
    const second = createClaudeStackModelRow({ model: "kimi-k3" });
    const draft = createClaudeStackModelRow();
    const onRowsChange = vi.fn();
    render(
      <FormShell>
        <ClaudeStackModelsField
          rows={[first, second, draft]}
          onRowsChange={onRowsChange}
          fetchedModels={[
            { id: "glm-5.2", ownedBy: null },
            { id: "kimi-k3", ownedBy: null },
            { id: "minimax-m3", ownedBy: null },
          ]}
          onFetchModels={vi.fn()}
          isFetchingModels={false}
        />
      </FormShell>,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.modelPicker.title" }),
    );
    const dialog = screen.getByRole("dialog");
    // 带 [1m] 的行按去掉标记后的 ID 认成已勾选
    expect(
      within(dialog).getByRole("checkbox", { name: "glm-5.2" }),
    ).toBeChecked();
    fireEvent.click(within(dialog).getByRole("checkbox", { name: "kimi-k3" }));
    fireEvent.click(
      within(dialog).getByRole("checkbox", { name: "minimax-m3" }),
    );
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "providerForm.modelPicker.apply",
      }),
    );

    expect(onRowsChange).toHaveBeenCalledTimes(1);
    const [rows] = onRowsChange.mock.calls[0];
    expect(rows.slice(0, 2)).toEqual([first, draft]);
    expect(rows).toHaveLength(3);
    expect(rows[2]).toMatchObject({ model: "minimax-m3" });
  });
});
