import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ComponentProps, PropsWithChildren } from "react";
import { useState } from "react";
import { useForm } from "react-hook-form";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodexFormFields } from "@/components/providers/forms/CodexFormFields";
import { Form } from "@/components/ui/form";
import type { FetchedModel } from "@/lib/api/model-fetch";
import type { CodexChatReasoning } from "@/types";

const modelFetchApiMock = vi.hoisted(() => ({
  fetchModelsForConfig: vi.fn(),
  fetchXaiOauthModels: vi.fn(),
  modelFetchRequestHeaders: vi.fn(),
  showFetchModelsError: vi.fn(),
}));

vi.mock("@/lib/api/model-fetch", () => ({
  fetchModelsForConfig: modelFetchApiMock.fetchModelsForConfig,
  fetchXaiOauthModels: modelFetchApiMock.fetchXaiOauthModels,
  modelFetchRequestHeaders: modelFetchApiMock.modelFetchRequestHeaders,
  showFetchModelsError: modelFetchApiMock.showFetchModelsError,
}));

// 上游 v4.0.0 起 CodexFormFields 内部使用 useModelMetadataFill（依赖
// QueryClientProvider）；本文件只测 model fetch 行为，mock 成 no-op 即可。
vi.mock("@/hooks/useModelMetadataFill", () => ({
  useModelMetadataFill: () => () => {},
}));

type CodexFormFieldsProps = ComponentProps<typeof CodexFormFields>;

const FormShell = ({ children }: PropsWithChildren) => {
  const form = useForm();

  return <Form {...form}>{children}</Form>;
};

// RTL's default whitespace normalizer collapses the multi-line placeholder,
// so query by a distinctive fragment instead of the exact string.
const HEADERS_PLACEHOLDER = /X-Provider/;

// The harness owns the headers override state so editing the textarea
// reproduces the exact user flow: an existing header override is changed
// while Base URL / API Key stay untouched.
const INITIAL_HEADERS_OVERRIDE = '{"X-Tenant": "tenant-1"}';

function baseCodexProps(): CodexFormFieldsProps {
  return {
    codexApiKey: "sk-test",
    onApiKeyChange: vi.fn(),
    category: "third_party",
    shouldShowApiKeyLink: false,
    websiteUrl: "",
    codexBaseUrl: "https://api.example.com",
    onBaseUrlChange: vi.fn(),
    isFullUrl: false,
    onFullUrlChange: vi.fn(),
    isEndpointModalOpen: false,
    onEndpointModalToggle: vi.fn(),
    autoSelect: false,
    onAutoSelectChange: vi.fn(),
    codexModel: "",
    onModelChange: vi.fn(),
    apiFormat: "openai_responses",
    onApiFormatChange: vi.fn(),
    anthropicAuthField: "ANTHROPIC_AUTH_TOKEN",
    onAnthropicAuthFieldChange: vi.fn(),
    impersonateClaudeCode: false,
    onImpersonateClaudeCodeChange: vi.fn(),
    maxOutputTokens: "",
    onMaxOutputTokensChange: vi.fn(),
    promptCacheRouting: "auto",
    onPromptCacheRoutingChange: vi.fn(),
    speedTestEndpoints: [],
    customUserAgent: "",
    onCustomUserAgentChange: vi.fn(),
    localProxyHeadersOverride: "",
    onLocalProxyHeadersOverrideChange: vi.fn(),
    localProxyBodyOverride: "",
    onLocalProxyBodyOverrideChange: vi.fn(),
    shouldShowSpeedTest: true,
  };
}

function renderCodexForm() {
  const baseProps = baseCodexProps();

  function Harness() {
    const [headersOverride, setHeadersOverride] = useState(
      INITIAL_HEADERS_OVERRIDE,
    );

    return (
      <FormShell>
        <CodexFormFields
          {...baseProps}
          localProxyHeadersOverride={headersOverride}
          onLocalProxyHeadersOverrideChange={setHeadersOverride}
        />
      </FormShell>
    );
  }

  return render(<Harness />);
}

function renderReasoningForm() {
  function Harness() {
    const [reasoning, setReasoning] = useState<CodexChatReasoning>({
      supportsThinking: true,
      supportsEffort: true,
      thinkingParam: "thinking",
      effortParam: "reasoning_effort",
      effortValueMode: "passthrough",
      outputFormat: "auto",
    });

    return (
      <FormShell>
        <output data-testid="reasoning-state">
          {JSON.stringify(reasoning)}
        </output>
        <CodexFormFields
          {...baseCodexProps()}
          apiFormat="openai_chat"
          codexChatReasoning={reasoning}
          onCodexChatReasoningChange={setReasoning}
        />
      </FormShell>
    );
  }

  return render(<Harness />);
}

async function fetchAndSettleModels(models: FetchedModel[]) {
  modelFetchApiMock.fetchModelsForConfig.mockResolvedValueOnce(models);

  fireEvent.click(
    screen.getByRole("button", { name: "providerForm.fetchModels" }),
  );

  await waitFor(() => {
    expect(modelFetchApiMock.fetchModelsForConfig).toHaveBeenCalled();
  });

  return screen.findByRole("button", { name: "Select model" });
}

function changeHeadersOverride(value: string) {
  fireEvent.change(screen.getByPlaceholderText(HEADERS_PLACEHOLDER), {
    target: { value },
  });
}

describe("CodexFormFields model fetch identity", () => {
  beforeEach(() => {
    modelFetchApiMock.fetchModelsForConfig.mockReset();
    modelFetchApiMock.showFetchModelsError.mockReset();
  });

  it("修改 Header 覆盖后清空已取到的模型列表", async () => {
    renderCodexForm();

    // Header 覆盖已有初值，高级区自动展开，textarea 可直接编辑
    await fetchAndSettleModels([{ id: "model-a", ownedBy: "vendor" }]);
    expect(
      screen.getByRole("button", { name: "Select model" }),
    ).toBeInTheDocument();

    changeHeadersOverride('{"X-Tenant": "tenant-2"}');

    await waitFor(() => {
      expect(screen.queryByRole("button", { name: "Select model" })).toBeNull();
    });
  });

  it("Header 覆盖在途变更后丢弃旧请求返回的模型", async () => {
    renderCodexForm();

    let resolveFetch!: (models: FetchedModel[]) => void;
    modelFetchApiMock.fetchModelsForConfig.mockImplementationOnce(
      () =>
        new Promise<FetchedModel[]>((resolve) => {
          resolveFetch = resolve;
        }),
    );

    fireEvent.click(
      screen.getByRole("button", { name: "providerForm.fetchModels" }),
    );
    await waitFor(() => {
      expect(modelFetchApiMock.fetchModelsForConfig).toHaveBeenCalled();
    });

    changeHeadersOverride('{"X-Tenant": "tenant-2"}');

    // 旧身份的响应此时才落地。先正向等待响应链走完（按钮恢复可用），
    // 再断言模型未上榜——否则否定断言会在响应落地前抢先通过。
    resolveFetch([{ id: "stale-model", ownedBy: "vendor" }]);

    const fetchButton = screen.getByRole("button", {
      name: "providerForm.fetchModels",
    });
    await waitFor(() => {
      expect(fetchButton).toBeEnabled();
    });

    expect(screen.queryByRole("button", { name: "Select model" })).toBeNull();
  });
});

describe("CodexFormFields reasoning effort", () => {
  it("restores the default parameter after effort support is disabled and re-enabled", () => {
    renderReasoningForm();

    const toggle = screen.getByRole("switch", {
      name: "支持思考等级",
    });
    fireEvent.click(toggle);
    expect(screen.getByTestId("reasoning-state")).toHaveTextContent(
      '"supportsEffort":false',
    );
    expect(screen.getByTestId("reasoning-state")).toHaveTextContent(
      '"effortParam":"none"',
    );

    fireEvent.click(toggle);
    expect(screen.getByTestId("reasoning-state")).toHaveTextContent(
      '"supportsEffort":true',
    );
    expect(screen.getByTestId("reasoning-state")).toHaveTextContent(
      '"effortParam":"reasoning_effort"',
    );
  });
});
