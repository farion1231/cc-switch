import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  ProviderForm,
  type ProviderFormValues,
} from "@/components/providers/forms/ProviderForm";
import type { ProviderCategory, ProviderMeta } from "@/types";
import { createTestQueryClient } from "../utils/testQueryClient";

const settingsState = vi.hoisted(() => ({ enableStackMode: true }));

// 聚合版式只看应用当前是不是聚合模式（和应用页同一份 get_app_mode 数据）
vi.mock("@/lib/query/proxy", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/query/proxy")>();
  return {
    ...actual,
    useAppMode: () => ({
      data: {
        mode: settingsState.enableStackMode ? "stack" : "direct",
        attached: false,
        routeProviderId: null,
        directProviderId: null,
      },
    }),
  };
});

vi.mock("@/components/providers/forms/CodexConfigEditor", () => ({
  default: () => <div data-testid="codex-config-editor" />,
}));

vi.mock("@/components/providers/forms/hooks", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/components/providers/forms/hooks")>();
  const signedOut = {
    isAuthenticated: false,
    isStatusSuccess: true,
    isStatusError: false,
    accounts: [],
  };
  return {
    ...actual,
    useCopilotAuth: () => signedOut,
    useCodexOauth: () => signedOut,
    useXaiOauth: () => signedOut,
  };
});

const CLAUDE_ENV = {
  ANTHROPIC_BASE_URL: "https://relay.example",
  ANTHROPIC_AUTH_TOKEN: "sk-test",
  ANTHROPIC_MODEL: "m-a",
  ANTHROPIC_DEFAULT_SONNET_MODEL: "m-s[1M]",
  ANTHROPIC_DEFAULT_SONNET_MODEL_NAME: "Model S",
};

const CODEX_CONFIG =
  'model_provider = "custom"\nmodel = "m-1"\n\n[model_providers.custom]\nname = "custom"\nbase_url = "https://relay.example/v1"\nexperimental_bearer_token = "sk-test"\n';

function renderForm(
  appId: "claude" | "codex",
  onSubmit: (values: ProviderFormValues) => void,
  options: {
    meta?: ProviderMeta;
    category?: ProviderCategory;
    codexCatalog?: Array<{ model: string; displayName?: string }>;
  } = {},
) {
  const settingsConfig =
    appId === "claude"
      ? { env: CLAUDE_ENV }
      : {
          auth: {},
          config: CODEX_CONFIG,
          ...(options.codexCatalog
            ? { modelCatalog: { models: options.codexCatalog } }
            : {}),
        };
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ProviderForm
        appId={appId}
        providerId="relay"
        submitLabel="save-provider"
        onSubmit={onSubmit}
        onCancel={vi.fn()}
        initialData={{
          name: "Relay",
          category: options.category ?? "third_party",
          settingsConfig,
          meta: options.meta,
        }}
      />
    </QueryClientProvider>,
  );
}

/** 每一行 ★ 按钮的 aria-label，按行的顺序。 */
function starLabels() {
  return screen
    .getAllByRole("button", { name: /默认模型$/ })
    .map((button) => button.getAttribute("aria-label"));
}

async function save(onSubmit: ReturnType<typeof vi.fn>) {
  fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
  await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
  const values = onSubmit.mock.calls[0][0] as ProviderFormValues;
  return { values, settings: JSON.parse(values.settingsConfig) };
}

describe("ProviderForm Stack layout (Claude Code)", () => {
  beforeEach(() => {
    settingsState.enableStackMode = true;
  });

  it("开了 Stack 设置时默认用简化面板：模型列表跟着映射，没有映射和配置编辑器", () => {
    renderForm("claude", vi.fn());

    expect(screen.getByText("模型列表")).toBeInTheDocument();
    expect(screen.getByDisplayValue("m-a")).toBeInTheDocument();
    expect(screen.getByDisplayValue("m-s")).toBeInTheDocument();
    expect(screen.getByDisplayValue("Model S")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "默认模型" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.queryByText("providerForm.modelMappingLabel")).toBeNull();
    expect(screen.queryByText("默认兜底模型")).toBeNull();
  });

  it("没动过列表时保存不写 stackModels，ANTHROPIC_MODEL 不变", async () => {
    const onSubmit = vi.fn();
    renderForm("claude", onSubmit);

    const { values, settings } = await save(onSubmit);
    expect(values.meta?.stackModels).toBeUndefined();
    expect(settings.env.ANTHROPIC_MODEL).toBe("m-a");
  });

  it("设为默认把这一行移到第一位，保存时 ANTHROPIC_MODEL 跟着第一行", async () => {
    const onSubmit = vi.fn();
    renderForm("claude", onSubmit);

    fireEvent.click(screen.getByRole("button", { name: "设为默认模型" }));
    const inputs = screen.getAllByPlaceholderText("例如 deepseek-v4-pro");
    expect(inputs.map((input) => (input as HTMLInputElement).value)).toEqual([
      "m-s",
      "m-a",
    ]);

    const { values, settings } = await save(onSubmit);
    expect(values.meta?.stackModels).toEqual([
      { model: "m-s", displayName: "Model S", oneM: true },
      { model: "m-a" },
    ]);
    expect(settings.env.ANTHROPIC_MODEL).toBe("m-s[1M]");
    expect(settings.env.ANTHROPIC_DEFAULT_SONNET_MODEL).toBe("m-s[1M]");
  });

  it("行里配了列表就用它；编辑后保存，第一行没变时 ANTHROPIC_MODEL 也不变", async () => {
    const onSubmit = vi.fn();
    renderForm("claude", onSubmit, {
      meta: { stackModels: [{ model: "m-b", displayName: "B" }] },
    });

    expect(screen.getByDisplayValue("m-b")).toBeInTheDocument();
    expect(screen.queryByDisplayValue("m-a")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "添加模型" }));
    const inputs = screen.getAllByPlaceholderText("例如 deepseek-v4-pro");
    fireEvent.change(inputs[1], { target: { value: "m-c[1M]" } });

    const { values, settings } = await save(onSubmit);
    expect(values.meta?.stackModels).toEqual([
      { model: "m-b", displayName: "B" },
      { model: "m-c", oneM: true },
    ]);
    expect(settings.env.ANTHROPIC_MODEL).toBe("m-a");
  });

  it("在简化表单换了默认，切到完整表单保存时 ANTHROPIC_MODEL 也跟着", async () => {
    const onSubmit = vi.fn();
    renderForm("claude", onSubmit);

    fireEvent.click(screen.getByRole("button", { name: "设为默认模型" }));
    fireEvent.click(screen.getByRole("button", { name: "显示完整表单" }));

    const { values, settings } = await save(onSubmit);
    expect(values.meta?.stackModels?.[0]).toEqual({
      model: "m-s",
      displayName: "Model S",
      oneM: true,
    });
    expect(settings.env.ANTHROPIC_MODEL).toBe("m-s[1M]");
  });

  it("清空列表后保存为空列表，不再回落到模型映射", async () => {
    const onSubmit = vi.fn();
    renderForm("claude", onSubmit);

    // 从最后一行删起：第一行一直没变，删光了也不碰 ANTHROPIC_MODEL。
    for (const button of screen
      .getAllByRole("button", { name: "common.delete" })
      .reverse()) {
      fireEvent.click(button);
    }
    expect(screen.queryByDisplayValue("m-a")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "save-provider" }));
    fireEvent.click(await screen.findByRole("button", { name: "仍要保存" }));

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
    const values = onSubmit.mock.calls[0][0] as ProviderFormValues;
    expect(values.meta?.stackModels).toEqual([]);
    expect(JSON.parse(values.settingsConfig).env.ANTHROPIC_MODEL).toBe("m-a");
  });

  it("可以切到完整表单再切回来，改过的列表还在", () => {
    renderForm("claude", vi.fn());

    fireEvent.click(screen.getByRole("button", { name: "设为默认模型" }));
    fireEvent.click(screen.getByRole("button", { name: "显示完整表单" }));
    expect(
      screen.getByText("providerForm.modelMappingLabel"),
    ).toBeInTheDocument();
    expect(screen.queryByText("模型列表")).toBeNull();

    fireEvent.click(
      screen.getByRole("button", { name: "返回聚合模式的简化表单" }),
    );
    const inputs = screen.getAllByPlaceholderText("例如 deepseek-v4-pro");
    expect(inputs.map((input) => (input as HTMLInputElement).value)).toEqual([
      "m-s",
      "m-a",
    ]);
  });

  it("没开 Stack 设置时用完整表单，保存时原样保留列表", async () => {
    settingsState.enableStackMode = false;
    const onSubmit = vi.fn();
    renderForm("claude", onSubmit, {
      meta: { stackModels: [{ model: "m-b", oneM: true }] },
    });

    expect(screen.queryByText("模型列表")).toBeNull();
    expect(screen.queryByRole("button", { name: "显示完整表单" })).toBeNull();
    const { values, settings } = await save(onSubmit);
    expect(values.meta?.stackModels).toEqual([{ model: "m-b", oneM: true }]);
    expect(settings.env.ANTHROPIC_MODEL).toBe("m-a");
  });

  it("官方供应商不用简化面板", () => {
    renderForm("claude", vi.fn(), { category: "official" });
    expect(screen.queryByRole("button", { name: "显示完整表单" })).toBeNull();
    expect(screen.queryByText("模型列表")).toBeNull();
  });
});

describe("ProviderForm Stack layout (Codex)", () => {
  beforeEach(() => {
    settingsState.enableStackMode = true;
  });

  it("简化面板：模型列表在外面，没有默认模型字段和配置编辑器", () => {
    renderForm("codex", vi.fn());

    expect(screen.getByText("模型列表")).toBeInTheDocument();
    expect(
      screen.getByText(
        "未配置模型：聚合模式下只发布这家的默认模型（config.toml 的 model）。",
      ),
    ).toBeInTheDocument();
    expect(document.getElementById("codexDefaultModel")).toBeNull();
    expect(screen.queryByTestId("codex-config-editor")).toBeNull();
  });

  it("列表第一行是默认模型，保存时写进 model", async () => {
    const onSubmit = vi.fn();
    renderForm("codex", onSubmit, {
      codexCatalog: [
        { model: "m-1", displayName: "M1" },
        { model: "m-2", displayName: "M2" },
      ],
    });

    fireEvent.click(screen.getByRole("button", { name: "设为默认模型" }));
    const { settings } = await save(onSubmit);
    expect(
      settings.modelCatalog.models.map(
        (model: { model: string }) => model.model,
      ),
    ).toEqual(["m-2", "m-1"]);
    expect(settings.config).toContain('model = "m-2"');
  });

  it("加载时 ★ 跟着原来的 model，不改动保存时 model 和顺序都不变", async () => {
    const onSubmit = vi.fn();
    renderForm("codex", onSubmit, {
      codexCatalog: [
        { model: "m-2", displayName: "M2" },
        { model: "m-1", displayName: "M1" },
      ],
    });

    expect(starLabels()).toEqual(["设为默认模型", "默认模型"]);

    const { settings } = await save(onSubmit);
    expect(
      settings.modelCatalog.models.map(
        (model: { model: string }) => model.model,
      ),
    ).toEqual(["m-2", "m-1"]);
    expect(settings.config).toContain('model = "m-1"');
  });

  it("在简化表单换了默认，切到完整表单保存时 model 也跟着", async () => {
    const onSubmit = vi.fn();
    renderForm("codex", onSubmit, {
      codexCatalog: [
        { model: "m-1", displayName: "M1" },
        { model: "m-2", displayName: "M2" },
      ],
    });

    fireEvent.click(screen.getByRole("button", { name: "设为默认模型" }));
    fireEvent.click(screen.getByRole("button", { name: "显示完整表单" }));
    expect(
      (document.getElementById("codexDefaultModel") as HTMLInputElement).value,
    ).toBe("m-2");

    const { settings } = await save(onSubmit);
    expect(settings.config).toContain('model = "m-2"');
  });

  it("默认模型不在列表里时提示，可以加进列表", async () => {
    const onSubmit = vi.fn();
    renderForm("codex", onSubmit, {
      codexCatalog: [{ model: "m-2", displayName: "M2" }],
    });

    expect(starLabels()).toEqual(["设为默认模型"]);
    expect(
      screen.getByText(
        "默认模型 m-1 不在列表里：Codex 默认仍请求它，但 /model 里没有这一项。",
      ),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "加入列表" }));
    expect(starLabels()).toEqual(["默认模型", "设为默认模型"]);

    const { settings } = await save(onSubmit);
    expect(
      settings.modelCatalog.models.map(
        (model: { model: string }) => model.model,
      ),
    ).toEqual(["m-1", "m-2"]);
    expect(settings.config).toContain('model = "m-1"');
  });

  it("删掉默认那行，model 换成剩下的第一行", async () => {
    const onSubmit = vi.fn();
    renderForm("codex", onSubmit, {
      codexCatalog: [
        { model: "m-2", displayName: "M2" },
        { model: "m-1", displayName: "M1" },
        { model: "m-3", displayName: "M3" },
      ],
    });

    fireEvent.click(screen.getAllByRole("button", { name: "删除" })[1]);
    expect(starLabels()).toEqual(["默认模型", "设为默认模型"]);

    const { settings } = await save(onSubmit);
    expect(settings.config).toContain('model = "m-2"');
  });

  it("没开 Stack 设置时仍是完整表单", () => {
    settingsState.enableStackMode = false;
    renderForm("codex", vi.fn());
    expect(screen.queryByText("模型列表")).toBeNull();
    expect(document.getElementById("codexDefaultModel")).not.toBeNull();
    expect(screen.getByTestId("codex-config-editor")).toBeInTheDocument();
  });
});
