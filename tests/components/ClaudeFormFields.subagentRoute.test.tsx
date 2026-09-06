import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState, type ComponentProps, type PropsWithChildren } from "react";
import { useForm } from "react-hook-form";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { ClaudeFormFields } from "@/components/providers/forms/ClaudeFormFields";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
import { Form } from "@/components/ui/form";
import type { AppProxyConfig } from "@/types/proxy";
import type { ProvidersQueryData } from "@/lib/query/queries";

// ---------------------------------------------------------------------------
// 共享 mock
// ---------------------------------------------------------------------------

const modelFetchApiMock = vi.hoisted(() => ({
  fetchCodexOauthModels: vi.fn(),
  fetchModelsForConfig: vi.fn(),
  showFetchModelsError: vi.fn(),
}));

vi.mock("@/lib/api/model-fetch", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  fetchCodexOauthModels: modelFetchApiMock.fetchCodexOauthModels,
  fetchModelsForConfig: modelFetchApiMock.fetchModelsForConfig,
  showFetchModelsError: modelFetchApiMock.showFetchModelsError,
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/components/providers/forms/CopilotAuthSection", () => ({
  CopilotAuthSection: () => <div data-testid="copilot-auth-section" />,
}));
vi.mock("@/components/providers/forms/CodexOAuthSection", () => ({
  CodexOAuthSection: () => <div data-testid="codex-oauth-section" />,
}));

// ---------------------------------------------------------------------------
// ClaudeFormFields（展示层）契约测试
// ---------------------------------------------------------------------------

type ClaudeFormFieldsProps = ComponentProps<typeof ClaudeFormFields>;

const FormShell = ({ children }: PropsWithChildren) => {
  const form = useForm();
  return <Form {...form}>{children}</Form>;
};

const buildFieldsProps = (
  overrides: Partial<ClaudeFormFieldsProps> = {},
): ClaudeFormFieldsProps => {
  return {
    shouldShowApiKey: false,
    apiKey: "",
    onApiKeyChange: vi.fn(),
    category: "third_party",
    shouldShowApiKeyLink: false,
    websiteUrl: "",
    templateValueEntries: [],
    templateValues: {},
    templatePresetName: "",
    onTemplateValueChange: vi.fn(),
    shouldShowSpeedTest: false,
    baseUrl: "",
    onBaseUrlChange: vi.fn(),
    isEndpointModalOpen: false,
    onEndpointModalToggle: vi.fn(),
    autoSelect: false,
    onAutoSelectChange: vi.fn(),
    shouldShowModelSelector: true,
    claudeModel: "fallback-model",
    defaultHaikuModel: "",
    defaultHaikuModelName: "",
    defaultSonnetModel: "",
    defaultSonnetModelName: "",
    defaultOpusModel: "",
    defaultOpusModelName: "",
    defaultFableModel: "",
    defaultFableModelName: "",
    subagentModel: "",
    onModelChange: vi.fn(),
    speedTestEndpoints: [],
    apiFormat: "anthropic",
    onApiFormatChange: vi.fn(),
    apiKeyField: "ANTHROPIC_AUTH_TOKEN",
    onApiKeyFieldChange: vi.fn(),
    isFullUrl: false,
    onFullUrlChange: vi.fn(),
    customUserAgent: "",
    onCustomUserAgentChange: vi.fn(),
    localProxyHeadersOverride: "",
    onLocalProxyHeadersOverrideChange: vi.fn(),
    localProxyBodyOverride: "",
    onLocalProxyBodyOverrideChange: vi.fn(),
    ...overrides,
  };
};

const renderFields = (overrides: Partial<ClaudeFormFieldsProps> = {}) => {
  const props = buildFieldsProps(overrides);

  return render(
    <FormShell>
      <ClaudeFormFields {...props} />
    </FormShell>,
  );
};

const routeProps = (
  overrides: Partial<ClaudeFormFieldsProps> = {},
): Partial<ClaudeFormFieldsProps> => ({
  providerId: "self",
  subagentRouteTarget: "",
  onSubagentRouteTargetChange: vi.fn(),
  subagentRouteModel: "",
  onSubagentRouteModelChange: vi.fn(),
  subagentRouteOptions: [
    { id: "b", name: "Provider B" },
    { id: "c", name: "Provider C" },
  ],
  subagentRouteTargetEndpoint: {
    baseUrl: "https://b.example.com",
    apiKey: "sk-b",
    isFullUrl: false,
  },
  subagentRouteTargetExists: true,
  subagentRouteTakeoverActive: true,
  ...overrides,
});

/** 受控草稿挂载壳：勾选/输入都真实更新草稿并回灌 props，模拟真实受控流 */
const StatefulRouteFields = ({ initialModel }: { initialModel: string }) => {
  const [routeModel, setRouteModel] = useState(initialModel);
  const props = buildFieldsProps(
    routeProps({
      subagentRouteTarget: "b",
      subagentRouteModel: routeModel,
      onSubagentRouteModelChange: setRouteModel,
    }),
  );
  return (
    <FormShell>
      <ClaudeFormFields {...props} />
    </FormShell>
  );
};

describe("ClaudeFormFields SubAgent 路由行", () => {
  beforeEach(() => {
    modelFetchApiMock.fetchModelsForConfig.mockResolvedValue([]);
    modelFetchApiMock.showFetchModelsError.mockClear();
    Element.prototype.scrollIntoView = vi.fn();
  });

  it("下拉包含「本供应商」与候选供应商", async () => {
    const user = userEvent.setup();
    renderFields(routeProps());

    // 行标签与模型映射区其他行同源（modelRoleSubagent），而非独立路由标签
    expect(screen.getByText("providerForm.modelRoleSubagent")).toBeDefined();

    await user.click(
      screen.getByTestId("subagent-route-provider-trigger"),
    );

    const options = await screen.findAllByRole("option");
    const texts = options.map((o) => o.textContent ?? "");
    expect(texts).toContain("providerForm.subagentRouteSelfTarget");
    expect(texts).toContain("Provider B");
    expect(texts).toContain("Provider C");
  });

  it("target=B 时模型输入写路由草稿回调，不触碰 env 字段", async () => {
    const onSubagentRouteModelChange = vi.fn();
    const onModelChange = vi.fn();
    renderFields(
      routeProps({
        subagentRouteTarget: "b",
        onSubagentRouteModelChange,
        onModelChange,
      }),
    );

    const routeInput = document.getElementById(
      "claudeCodeSubagentRouteModel",
    ) as HTMLInputElement | null;
    expect(routeInput).not.toBeNull();
    expect(
      document.getElementById("claudeCodeSubagentModel"),
    ).toBeNull();

    fireEvent.change(routeInput!, {
      target: { value: "glm-5.5-flash" },
    });

    expect(onSubagentRouteModelChange).toHaveBeenCalledWith("glm-5.5-flash");
    expect(onModelChange).not.toHaveBeenCalled();
  });

  it("target=本供应商 时模型输入仍走 env 字段回调", () => {
    const onSubagentRouteModelChange = vi.fn();
    const onModelChange = vi.fn();
    renderFields(
      routeProps({
        subagentRouteTarget: "",
        onSubagentRouteModelChange,
        onModelChange,
      }),
    );

    const envInput = document.getElementById(
      "claudeCodeSubagentModel",
    ) as HTMLInputElement | null;
    expect(envInput).not.toBeNull();
    expect(document.getElementById("claudeCodeSubagentRouteModel")).toBeNull();

    fireEvent.change(envInput!, { target: { value: "glm-5.5-flash" } });

    expect(onModelChange).toHaveBeenCalledWith(
      "CLAUDE_CODE_SUBAGENT_MODEL",
      "glm-5.5-flash",
    );
    expect(onSubagentRouteModelChange).not.toHaveBeenCalled();
  });

  it("target=B 时拉取模型透传 B 的 baseUrl/apiKey/isFullUrl/modelsUrl/customUserAgent", async () => {
    modelFetchApiMock.fetchModelsForConfig.mockResolvedValue([
      { id: "glm-5.5-flash", ownedBy: null },
    ]);
    const user = userEvent.setup();
    renderFields(
      routeProps({
        subagentRouteTarget: "b",
        subagentRouteTargetEndpoint: {
          baseUrl: "https://b.example.com",
          apiKey: "sk-b",
          isFullUrl: false,
          modelsUrl: "https://b.example.com/models",
          customUserAgent: "b-custom-ua",
        },
      }),
    );

    await user.click(screen.getByTitle("providerForm.fetchModels"));

    await waitFor(() => {
      expect(modelFetchApiMock.fetchModelsForConfig).toHaveBeenCalledWith(
        "https://b.example.com",
        "sk-b",
        false,
        "https://b.example.com/models",
        "b-custom-ua",
      );
    });
  });

  it("target=B 端点未命中预设时 modelsUrl/customUserAgent 传 undefined", async () => {
    const user = userEvent.setup();
    renderFields(routeProps({ subagentRouteTarget: "b" }));

    await user.click(screen.getByTitle("providerForm.fetchModels"));

    await waitFor(() => {
      expect(modelFetchApiMock.fetchModelsForConfig).toHaveBeenCalledWith(
        "https://b.example.com",
        "sk-b",
        false,
        undefined,
        undefined,
      );
    });
  });

  it("target=B 时 [1M] 开关切换路由模型草稿的 [1M] 标记", async () => {
    const onSubagentRouteModelChange = vi.fn();
    const onModelChange = vi.fn();
    const user = userEvent.setup();
    renderFields(
      routeProps({
        subagentRouteTarget: "b",
        subagentRouteModel: "glm-5.5-flash",
        onSubagentRouteModelChange,
        onModelChange,
      }),
    );

    const routeRow = document.getElementById(
      "claudeCodeSubagentRouteModel",
    )!.closest("div.grid") as HTMLElement;
    const oneMCheckbox = within(routeRow).getByRole("checkbox");
    expect(oneMCheckbox).not.toBeChecked();

    await user.click(oneMCheckbox);
    expect(onSubagentRouteModelChange).toHaveBeenCalledWith(
      "glm-5.5-flash[1M]",
    );
    expect(onModelChange).not.toHaveBeenCalled();
  });

  it("target=B 时 [1M] 已标记的模型开关为选中态，取消后去掉标记", async () => {
    const onSubagentRouteModelChange = vi.fn();
    const user = userEvent.setup();
    renderFields(
      routeProps({
        subagentRouteTarget: "b",
        subagentRouteModel: "glm-5.5-flash[1M]",
        onSubagentRouteModelChange,
      }),
    );

    const routeRow = document.getElementById(
      "claudeCodeSubagentRouteModel",
    )!.closest("div.grid") as HTMLElement;
    const oneMCheckbox = within(routeRow).getByRole("checkbox");
    expect(oneMCheckbox).toBeChecked();

    await user.click(oneMCheckbox);
    expect(onSubagentRouteModelChange).toHaveBeenCalledWith("glm-5.5-flash");
  });

  it("target=B 勾选 [1M] 后在输入框输入新模型名，标记按路由草稿保留", async () => {
    const user = userEvent.setup();
    render(<StatefulRouteFields initialModel="glm-5.5-flash" />);

    const routeRow = document.getElementById(
      "claudeCodeSubagentRouteModel",
    )!.closest("div.grid") as HTMLElement;
    const oneMCheckbox = within(routeRow).getByRole("checkbox");

    // 先勾选 [1M]：草稿带上标记
    await user.click(oneMCheckbox);
    await waitFor(() => {
      expect(
        document.getElementById("claudeCodeSubagentRouteModel"),
      ).toHaveValue("glm-5.5-flash[1M]");
    });

    // 再输入新模型名：标记不得丢失
    fireEvent.change(
      document.getElementById("claudeCodeSubagentRouteModel")!,
      { target: { value: "glm-5.5-air" } },
    );
    await waitFor(() => {
      expect(
        document.getElementById("claudeCodeSubagentRouteModel"),
      ).toHaveValue("glm-5.5-air[1M]");
    });
  });

  it("target=本供应商 时 [1M] 开关作用于 env 字段", async () => {
    const onSubagentRouteModelChange = vi.fn();
    const onModelChange = vi.fn();
    const user = userEvent.setup();
    renderFields(
      routeProps({
        subagentRouteTarget: "",
        subagentModel: "glm-5.5-flash",
        onSubagentRouteModelChange,
        onModelChange,
      }),
    );

    const routeRow = document.getElementById(
      "claudeCodeSubagentModel",
    )!.closest("div.grid") as HTMLElement;
    const oneMCheckbox = within(routeRow).getByRole("checkbox");
    expect(oneMCheckbox).not.toBeChecked();

    await user.click(oneMCheckbox);
    expect(onModelChange).toHaveBeenCalledWith(
      "CLAUDE_CODE_SUBAGENT_MODEL",
      "glm-5.5-flash[1M]",
    );
    expect(onSubagentRouteModelChange).not.toHaveBeenCalled();
  });

  it("规则目标已不存在时渲染失效警告", () => {
    renderFields(
      routeProps({ subagentRouteTarget: "gone", subagentRouteTargetExists: false }),
    );

    expect(
      screen.getByText("proxy.subagentRoute.targetMissingWarning"),
    ).toBeDefined();
  });

  it("代理接管未开启时渲染接管提示", () => {
    renderFields(routeProps({ subagentRouteTakeoverActive: false }));

    expect(
      screen.getByText("providerForm.subagentRouteTakeoverRequiredHint"),
    ).toBeDefined();
  });
});

// ---------------------------------------------------------------------------
// ProviderForm 集成：保存提交路由（完整 AppProxyConfig 展开）
// ---------------------------------------------------------------------------

const mockProxyConfig: AppProxyConfig = {
  appType: "claude",
  enabled: true,
  autoFailoverEnabled: false,
  maxRetries: 3,
  streamingFirstByteTimeout: 60,
  streamingIdleTimeout: 120,
  nonStreamingTimeout: 600,
  circuitFailureThreshold: 4,
  circuitSuccessThreshold: 2,
  circuitTimeoutSeconds: 60,
  circuitErrorRateThreshold: 0.6,
  circuitMinRequests: 10,
  subagentRoute: null,
};

const providersData: ProvidersQueryData = {
  providers: {
    self: {
      id: "self",
      name: "Self Provider",
      settingsConfig: {
        env: {
          ANTHROPIC_BASE_URL: "https://self.example.com",
          ANTHROPIC_AUTH_TOKEN: "sk-self",
        },
      },
    },
    b: {
      id: "b",
      name: "Provider B",
      settingsConfig: {
        env: {
          // 命中 DeepSeek 预设的 baseUrl（预设带 modelsUrl 覆写），用于拉取匹配断言
          ANTHROPIC_BASE_URL: "https://api.deepseek.com/anthropic",
          ANTHROPIC_AUTH_TOKEN: "sk-b",
        },
      },
    },
  },
  currentProviderId: "self",
};

const proxyUpdateMock = vi.fn();
const providerSubmitMock = vi.fn();

vi.mock("@/lib/query/proxy", () => ({
  useAppProxyConfig: () => ({
    data: mockProxyConfig,
    isLoading: false,
    error: null,
  }),
  useUpdateAppProxyConfig: () => ({
    mutateAsync: proxyUpdateMock.mockResolvedValue(undefined),
    isPending: false,
  }),
  useProxyTakeoverStatus: () => ({
    data: {
      claude: true,
      codex: false,
      gemini: false,
      grokbuild: false,
      opencode: false,
      openclaw: false,
      hermes: false,
    },
  }),
}));

vi.mock("@/lib/query/queries", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  useProvidersQuery: () => ({ data: providersData }),
  useSettingsQuery: () => ({ data: { commonConfigConfirmed: true } }),
}));

// jsdom 的 Range 缺少 getClientRects/getBoundingClientRect，CodeMirror 测量需要
if (typeof Range.prototype.getClientRects !== "function") {
  Range.prototype.getClientRects = () => [] as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect = () =>
    ({ x: 0, y: 0, width: 0, height: 0, top: 0, left: 0, right: 0, bottom: 0 }) as DOMRect;
}

const renderProviderForm = () => {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <ProviderForm
        appId="claude"
        providerId="self"
        submitLabel="Save"
        onSubmit={providerSubmitMock.mockResolvedValue(undefined)}
        onCancel={vi.fn()}
        initialData={{
          name: "Self Provider",
          websiteUrl: "",
          notes: "",
          settingsConfig: {
            env: {
              ANTHROPIC_BASE_URL: "https://self.example.com",
              ANTHROPIC_AUTH_TOKEN: "sk-self",
            },
          },
        }}
      />
    </QueryClientProvider>,
  );
};

const expandAdvanced = async (user: ReturnType<typeof userEvent.setup>) => {
  await user.click(
    screen.getByRole("button", { name: "providerForm.advancedOptionsToggle" }),
  );
};

const selectRouteTarget = async (
  user: ReturnType<typeof userEvent.setup>,
  name: string,
) => {
  await user.click(screen.getByTestId("subagent-route-provider-trigger"));
  await user.click(await screen.findByRole("option", { name }));
};

describe("ProviderForm SubAgent 路由保存", () => {
  beforeEach(() => {
    modelFetchApiMock.fetchModelsForConfig.mockResolvedValue([]);
    proxyUpdateMock.mockClear().mockResolvedValue(undefined);
    providerSubmitMock.mockClear().mockResolvedValue(undefined);
    mockProxyConfig.subagentRoute = null;
    Element.prototype.scrollIntoView = vi.fn();
  });

  it("目标下拉不含正在编辑的供应商", async () => {
    const user = userEvent.setup();
    renderProviderForm();
    await expandAdvanced(user);

    await user.click(screen.getByTestId("subagent-route-provider-trigger"));
    const options = await screen.findAllByRole("option");
    const texts = options.map((o) => o.textContent ?? "");
    expect(texts).toContain("providerForm.subagentRouteSelfTarget");
    expect(texts).toContain("Provider B");
    expect(texts.join(" ")).not.toContain("Self Provider");
  });

  it("target=B + 填模型：保存提交完整代理配置且 subagentRoute 指向 B，env 不被修改", async () => {
    const infoSpy = vi.spyOn(toast, "info").mockReturnValue("");
    const user = userEvent.setup();
    renderProviderForm();
    await expandAdvanced(user);

    await selectRouteTarget(user, "Provider B");
    fireEvent.change(document.getElementById("claudeCodeSubagentRouteModel")!, {
      target: { value: "glm-5.5-flash" },
    });

    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(proxyUpdateMock).toHaveBeenCalledTimes(1));
    const saved = proxyUpdateMock.mock.calls[0][0] as AppProxyConfig;
    // 整行 UPDATE：除 subagentRoute 外，其余代理配置字段必须原样保留
    expect(saved).toEqual({
      ...mockProxyConfig,
      subagentRoute: { providerId: "b", model: "glm-5.5-flash" },
    });
    // 供应商保存成功后才提交路由
    expect(providerSubmitMock).toHaveBeenCalledTimes(1);

    const submittedSettings = JSON.parse(
      providerSubmitMock.mock.calls[0][0].settingsConfig as string,
    ) as { env: Record<string, string> };
    expect(
      submittedSettings.env.CLAUDE_CODE_SUBAGENT_MODEL,
    ).toBeUndefined();

    // 接管生效中：保存后提示重启 Claude Code
    await waitFor(() => {
      expect(infoSpy).toHaveBeenCalledWith(
        "proxy.subagentRoute.restartHint",
        expect.objectContaining({ duration: 10000 }),
      );
    });
    infoSpy.mockRestore();
  });

  it("target=本供应商 + 填模型：env 写入且保存清除已有路由规则", async () => {
    mockProxyConfig.subagentRoute = { providerId: "b", model: "glm-5.5-flash" };
    const user = userEvent.setup();
    renderProviderForm();
    await expandAdvanced(user);

    // 已有规则回显为目标 B，用户切回「本供应商」再填写模型
    await selectRouteTarget(user, "providerForm.subagentRouteSelfTarget");
    fireEvent.change(document.getElementById("claudeCodeSubagentModel")!, {
      target: { value: "glm-5.5-flash" },
    });

    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(proxyUpdateMock).toHaveBeenCalledTimes(1));
    const saved = proxyUpdateMock.mock.calls[0][0] as AppProxyConfig;
    expect(saved.subagentRoute).toBeNull();

    const submittedSettings = JSON.parse(
      providerSubmitMock.mock.calls[0][0].settingsConfig as string,
    ) as { env: Record<string, string> };
    expect(submittedSettings.env.CLAUDE_CODE_SUBAGENT_MODEL).toBe(
      "glm-5.5-flash",
    );
  });

  it("target=B 拉取模型按预设匹配 modelsUrl 并透传 B 的自定义 UA（缺省为空串）", async () => {
    modelFetchApiMock.fetchModelsForConfig.mockResolvedValue([]);
    const user = userEvent.setup();
    renderProviderForm();
    await expandAdvanced(user);

    await selectRouteTarget(user, "Provider B");
    // 路由行的获取按钮（ModelInputWithFetch 的 title），非表单头部的文字按钮
    await user.click(screen.getByTitle("providerForm.fetchModels"));

    await waitFor(() => {
      expect(modelFetchApiMock.fetchModelsForConfig).toHaveBeenCalledWith(
        "https://api.deepseek.com/anthropic",
        "sk-b",
        false,
        // DeepSeek 预设的 modelsUrl 覆写
        "https://api.deepseek.com/models",
        // B 无 meta.customUserAgent 时为空串
        "",
      );
    });
  });

  it("路由草稿未变化时保存不提交代理配置", async () => {
    mockProxyConfig.subagentRoute = { providerId: "b", model: "kept" };
    const user = userEvent.setup();
    renderProviderForm();
    await expandAdvanced(user);

    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(providerSubmitMock).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(proxyUpdateMock).not.toHaveBeenCalled());
  });

  it("规则指向已删除供应商时渲染失效警告", async () => {
    mockProxyConfig.subagentRoute = { providerId: "gone", model: "x" };
    const user = userEvent.setup();
    renderProviderForm();
    await expandAdvanced(user);

    expect(
      screen.getByText("proxy.subagentRoute.targetMissingWarning"),
    ).toBeDefined();
  });
});
