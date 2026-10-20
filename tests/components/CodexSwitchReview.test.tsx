import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi } from "vitest";
import { CodexSwitchReview } from "@/components/providers/CodexSwitchReview";
import type { CodexSwitchPreview } from "@/lib/api/codexSelection";

const preview: CodexSwitchPreview = {
  revision: "snapshot",
  proxy: false,
  fields: [
    {
      id: "connection",
      label: "连接",
      before: "old",
      after: "new",
      selected: true,
    },
    {
      id: "catalog",
      label: "目录",
      before: "old",
      after: "new",
      selected: true,
    },
    {
      id: '["model"]',
      label: "model",
      before: "old",
      after: "new",
      selected: true,
    },
    {
      id: '["mcp_servers","demo"]',
      label: "MCP",
      before: "local",
      after: "import",
      selected: false,
    },
  ],
  models: [{ model: "new", reasoningLevels: ["medium", "high"] }],
  currentModels: [{ model: "old", reasoningLevels: ["low", "medium"] }],
  defaultModel: "new",
  currentModel: "old",
  currentEffort: "medium",
};
describe("Codex activation review", () => {
  it("allows an unrelated global edit with no current catalog or model", async () => {
    render(
      <CodexSwitchReview
        preview={{
          ...preview,
          currentModel: null,
          currentEffort: null,
          currentModels: [],
          fields: preview.fields.map((f) => ({
            ...f,
            selected: f.label === "MCP",
          })),
        }}
        onClose={vi.fn()}
      />,
    );
    expect(
      screen.getByRole("button", { name: "确认应用所选项" }),
    ).toBeEnabled();
    expect(
      screen.queryByRole("combobox", { name: "默认思考等级" }),
    ).not.toBeInTheDocument();
  });
  it("requires the connection field when leaving proxy mode", async () => {
    const close = vi.fn();
    render(
      <CodexSwitchReview
        preview={{ ...preview, exitToDirect: true }}
        onClose={close}
      />,
    );
    await userEvent.click(screen.getByRole("checkbox", { name: "连接" }));
    expect(
      screen.getByRole("button", { name: "确认应用所选项" }),
    ).toBeDisabled();
    await userEvent.click(screen.getByRole("checkbox", { name: "连接" }));
    await userEvent.click(
      screen.getByRole("button", { name: "确认应用所选项" }),
    );
    expect(close).toHaveBeenCalledWith(
      expect.objectContaining({ exitToDirect: true }),
    );
  });
  it("retains a compatible effort and leaves global fields unchecked", async () => {
    const close = vi.fn();
    render(<CodexSwitchReview preview={preview} onClose={close} />);
    expect(screen.getByRole("checkbox", { name: "MCP" })).not.toBeChecked();
    expect(screen.getByRole("combobox", { name: "默认思考等级" })).toHaveValue(
      "medium",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "确认应用所选项" }),
    );
    expect(close).toHaveBeenCalledWith({
      revision: "snapshot",
      selected: ["connection", "catalog", '["model"]'],
      model: "new",
      effort: "medium",
    });
  });
  it("requires an explicit choice for an unsupported previous effort", async () => {
    const close = vi.fn();
    render(
      <CodexSwitchReview
        preview={{ ...preview, currentEffort: "ultra" }}
        onClose={close}
      />,
    );
    expect(
      screen.getByRole("button", { name: "确认应用所选项" }),
    ).toBeDisabled();
    await userEvent.selectOptions(
      screen.getByRole("combobox", { name: "默认思考等级" }),
      "high",
    );
    expect(
      screen.getByRole("button", { name: "确认应用所选项" }),
    ).toBeEnabled();
  });
  it("cancels without an apply request", async () => {
    const close = vi.fn();
    render(<CodexSwitchReview preview={preview} onClose={close} />);
    await userEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(close).toHaveBeenCalledWith(null);
  });
  it("validates against the retained catalog when the new catalog is unchecked", async () => {
    render(<CodexSwitchReview preview={preview} onClose={vi.fn()} />);
    await userEvent.click(screen.getByRole("checkbox", { name: "目录" }));
    expect(
      screen.getByRole("button", { name: "确认应用所选项" }),
    ).toBeDisabled();
    await userEvent.selectOptions(
      screen.getByRole("combobox", { name: "默认模型" }),
      "old",
    );
    expect(
      screen.getByRole("button", { name: "确认应用所选项" }),
    ).toBeEnabled();
  });
});
