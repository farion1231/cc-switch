import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { renderWithQueryClient as render } from "../utils/testQueryClient";
import { useForm } from "react-hook-form";
import { useState, type PropsWithChildren } from "react";
import { describe, expect, it, vi } from "vitest";
import {
  ClaudeStackModelsField,
  createClaudeStackModelRow,
  fillClaudeStackModelWindow,
  type ClaudeStackModelRow,
} from "@/components/providers/forms/ClaudeStackModelsField";
import { Form } from "@/components/ui/form";
import { server } from "../msw/server";

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

describe("ClaudeStackModelsField context window fill", () => {
  it("fills windows of models picked from the fetched list via models.dev", async () => {
    server.use(
      http.post("http://tauri.local/fetch_models_dev", () =>
        HttpResponse.json(
          JSON.stringify({
            relay: {
              api: "https://relay.example/v1",
              models: {
                "kimi-k3": { limit: { context: 256000 } },
                "glm-5.2": { limit: { context: 1000000 } },
              },
            },
          }),
        ),
      ),
    );
    const kept = createClaudeStackModelRow({
      model: "minimax-m3",
      contextWindow: 128000,
    });
    let latest: ClaudeStackModelRow[] = [];
    const Harness = () => {
      const [rows, setRows] = useState([kept]);
      latest = rows;
      return (
        <ClaudeStackModelsField
          rows={rows}
          onRowsChange={setRows}
          fetchedModels={[
            { id: "kimi-k3", ownedBy: null },
            { id: "glm-5.2", ownedBy: null },
            { id: "minimax-m3", ownedBy: null },
          ]}
          onFetchModels={vi.fn()}
          isFetchingModels={false}
          baseUrl="https://relay.example"
        />
      );
    };
    render(
      <FormShell>
        <Harness />
      </FormShell>,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.modelPicker.title" }),
    );
    const dialog = screen.getByRole("dialog");
    fireEvent.click(within(dialog).getByRole("checkbox", { name: "kimi-k3" }));
    fireEvent.click(within(dialog).getByRole("checkbox", { name: "glm-5.2" }));
    fireEvent.click(
      within(dialog).getByRole("button", {
        name: "providerForm.modelPicker.apply",
      }),
    );

    await waitFor(() =>
      expect(latest.map(({ rowId: _, ...row }) => row)).toEqual([
        // 已经填过的窗口不动
        { model: "minimax-m3", contextWindow: 128000 },
        { model: "kimi-k3", contextWindow: 256000 },
        // 不小于 1M 的按 1M 标
        { model: "glm-5.2", oneM: true, contextWindow: undefined },
      ]),
    );
    // 自定义输入框里直接显示补上的值
    expect(screen.getByDisplayValue("256000")).toBeInTheDocument();
  });

  it("fills rows already in the list once the model list is fetched", async () => {
    server.use(
      http.post("http://tauri.local/fetch_models_dev", () =>
        HttpResponse.json(
          JSON.stringify({
            relay: {
              api: "https://relay.example/v1",
              models: {
                "kimi-k3": { limit: { context: 256000 } },
                "glm-5.2": { limit: { context: 200000 } },
                "minimax-m3": { limit: { context: 1000000 } },
              },
            },
          }),
        ),
      ),
    );
    const initial = [
      createClaudeStackModelRow({ model: "kimi-k3" }),
      createClaudeStackModelRow({ model: "glm-5.2" }),
      createClaudeStackModelRow({ model: "minimax-m3", contextWindow: 384 }),
    ];
    let latest: ClaudeStackModelRow[] = [];
    const Harness = () => {
      const [rows, setRows] = useState(initial);
      const [fetched, setFetched] = useState<
        { id: string; ownedBy: string | null }[]
      >([]);
      latest = rows;
      return (
        <ClaudeStackModelsField
          rows={rows}
          onRowsChange={setRows}
          fetchedModels={fetched}
          onFetchModels={() =>
            setFetched([
              { id: "kimi-k3", ownedBy: null },
              { id: "glm-5.2", ownedBy: null },
              { id: "minimax-m3", ownedBy: null },
            ])
          }
          isFetchingModels={false}
          baseUrl="https://relay.example"
        />
      );
    };
    render(
      <FormShell>
        <Harness />
      </FormShell>,
    );
    // 还没拉过列表：不补
    expect(latest[0].contextWindow).toBeUndefined();

    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );

    await waitFor(() =>
      expect(latest.map(({ rowId: _, ...row }) => row)).toEqual([
        { model: "kimi-k3", contextWindow: 256000 },
        { model: "glm-5.2", contextWindow: 200000 },
        // 填过的不动，填的是多少都照用
        { model: "minimax-m3", contextWindow: 384 },
      ]),
    );
  });

  it("only fills rows whose window is still unset", () => {
    const row = createClaudeStackModelRow({ model: "m" });
    expect(fillClaudeStackModelWindow(row, {})).toBeNull();
    expect(
      fillClaudeStackModelWindow(row, { contextWindow: 200000 }),
    ).toMatchObject({ contextWindow: 200000 });
    expect(
      fillClaudeStackModelWindow(
        createClaudeStackModelRow({ model: "m[1m]" }),
        { contextWindow: 200000 },
      ),
    ).toBeNull();
    expect(
      fillClaudeStackModelWindow(
        createClaudeStackModelRow({ model: "m", contextWindow: 64000 }),
        { contextWindow: 200000 },
      ),
    ).toBeNull();
  });

  it.each(["256K", ""])(
    "does not fill over the user's window draft %j",
    (input) => {
      const row = {
        ...createClaudeStackModelRow({ model: "m" }),
        contextWindowInput: input,
      };
      expect(
        fillClaudeStackModelWindow(row, { contextWindow: 1000000 }),
      ).toBeNull();
    },
  );
});
