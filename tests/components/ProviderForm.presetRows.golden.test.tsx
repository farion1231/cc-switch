/**
 * 重构前的金标：锁定「从预设新增供应商时写进 DB 的行」。
 *
 * 渲染真实的 ProviderForm（新增模式，不传 initialData），在预设选择器里点中预设、
 * 填 API Key（有模板变量的一并填上）、提交，把 onSubmit 收到的载荷做稳定化后存快照：
 * - 先 JSON 往返一次（与 Tauri IPC 序列化一致：值为 undefined 的键不会进 DB）；
 * - settingsConfig 从 JSON 字符串解析成对象，便于读 diff；
 * - presetId 是按预设列表下标生成的（claude-N / codex-N），下标替换成 <index>，
 *   另行断言它仍指回被点中的预设，避免新增预设导致无关的快照抖动；
 * - meta 原样保留（commonConfigEnabled / apiFormat / endpointAutoSelect 等都在快照里）；
 * - apiKeyLocations 列出测试 Key 在行里出现的所有位置（含 TOML 字符串内），审 Key 落点用。
 *
 * 快照变了 = 行的形状变了：必须能说清原因再更新。
 * 更新方式：pnpm vitest run tests/components/ProviderForm.presetRows.golden.test.tsx -u
 *
 * 替身（均不改变行内容）：
 * - JsonEditor → textarea：CodeMirror 在 jsdom 下不可靠，它只回显 value；
 * - CodexOAuthSection → 空桩：新建官方卡不动登录方式 = 跟随 Codex 当前登录；
 * - OAuth hooks → 未登录；通用配置 hooks → 未勾选、片段为空，快照与片段无关。
 *
 * 预设取舍：
 * - Claude「需要格式转换的 openai_chat」：OpenRouter 现在走 Anthropic 原生，
 *   GitHub Copilot 必须 OAuth 登录，选 Nvidia；
 * - Claude「上下文窗口类键」：Kimi For Coding（同时带 CLAUDE_CODE_MAX_CONTEXT_TOKENS
 *   和 CLAUDE_CODE_AUTO_COMPACT_WINDOW）；
 * - Codex「原生 Responses 第三方」：xAI (Grok)（category=third_party、显式
 *   apiFormat=openai_responses、目录带原生 Responses 专用字段）；
 * - Codex「openai_chat + 模型目录」：Nvidia（另带 codexChatReasoning）。
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  ProviderForm,
  type ProviderFormValues,
} from "@/components/providers/forms/ProviderForm";
import { providerPresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import {
  PRESET_FAMILIES,
  type PresetFamilyInfo,
} from "@/config/presetFamilies";
import { createTestQueryClient } from "../utils/testQueryClient";

const TEST_API_KEY = "sk-golden-test";
const SUBMIT_LABEL = "save-provider";

const toastMocks = vi.hoisted(() => ({
  error: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    error: toastMocks.error,
    success: vi.fn(),
    info: vi.fn(),
    warning: vi.fn(),
  },
}));

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
  }: {
    value: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      data-testid="json-editor"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

vi.mock("@/components/providers/forms/CodexOAuthSection", () => ({
  CodexOAuthSection: () => <div data-testid="codex-oauth-section" />,
}));

vi.mock("@/components/providers/forms/hooks", async (importOriginal) => {
  const actual =
    await importOriginal<typeof import("@/components/providers/forms/hooks")>();
  return {
    ...actual,
    useCopilotAuth: () => ({
      isAuthenticated: false,
      isStatusSuccess: true,
      isStatusError: false,
      accounts: [],
    }),
    useCodexOauth: () => ({
      isAuthenticated: false,
      isStatusSuccess: true,
      isStatusError: false,
      defaultAccountId: null,
      accounts: [],
    }),
    useXaiOauth: () => ({
      isAuthenticated: false,
      accounts: [],
    }),
  };
});

vi.mock("@/lib/query", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/query")>();
  return {
    ...actual,
    useSettingsQuery: () => ({
      data: { commonConfigConfirmed: true },
    }),
  };
});

type GoldenAppId = "claude" | "codex";

interface GoldenCase {
  /** 预设显示名（即预设选择器里的按钮文本） */
  preset: string;
  /** 是否在 API Key 输入框里填测试 Key（官方预设没有 Key） */
  fillApiKey: boolean;
  /** 模板变量输入（key → 值），按表单要求填写 */
  templateValues?: Record<string, string>;
}

const CLAUDE_CASES: GoldenCase[] = [
  { preset: "Claude Official", fillApiKey: false },
  { preset: "RelaxyCode", fillApiKey: true },
  {
    preset: "AWS Bedrock (API Key)",
    fillApiKey: true,
    templateValues: { AWS_REGION: "us-east-1" },
  },
  { preset: "Nvidia", fillApiKey: true },
  { preset: "Kimi For Coding", fillApiKey: true },
  { preset: "E-FlowCode", fillApiKey: true },
];

const CODEX_CASES: GoldenCase[] = [
  { preset: "OpenAI Official", fillApiKey: false },
  { preset: "xAI (Grok)", fillApiKey: true },
  { preset: "Nvidia", fillApiKey: true },
  { preset: "E-FlowCode", fillApiKey: true },
];

const API_KEY_INPUT_ID: Record<GoldenAppId, string> = {
  claude: "apiKey",
  codex: "codexApiKey",
};

function presetNameAt(appId: GoldenAppId, index: number): string | undefined {
  // 与 ProviderForm 的 presetEntries 生成方式一致：Claude 先滤掉 hidden 再编号
  return appId === "claude"
    ? providerPresets.filter((preset) => !preset.hidden)[index]?.name
    : codexProviderPresets[index]?.name;
}

function renderForm(
  appId: GoldenAppId,
  onSubmit: (values: ProviderFormValues) => void,
) {
  const queryClient = createTestQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <ProviderForm
        appId={appId}
        submitLabel={SUBMIT_LABEL}
        onSubmit={onSubmit}
        onCancel={vi.fn()}
      />
    </QueryClientProvider>,
  );
}

function clickRow(rowName: string) {
  const matches = screen
    .getAllByRole("button")
    .filter(
      (button) =>
        button.querySelector("span.truncate")?.textContent === rowName,
    );
  expect(matches, `预设按钮「${rowName}」应唯一`).toHaveLength(1);
  fireEvent.click(matches[0]);
}

function clickButtonIn(group: HTMLElement, label: string) {
  const button = Array.from(group.querySelectorAll("button")).find(
    (item) => item.textContent === label,
  );
  expect(button, `版本按钮「${label}」应存在`).toBeDefined();
  fireEvent.click(button!);
}

/**
 * 同一家的多个版本合成一行：先点那一行，再选版本。套餐、地区都在变（完整网格）时
 * 分别点套餐和地区；只有一维在变时「版本」里的按钮只写那一维。
 */
function clickPreset(appId: GoldenAppId, presetName: string) {
  const preset = (
    appId === "claude" ? providerPresets : codexProviderPresets
  ).find((item) => item.name === presetName);
  if (!preset?.family) {
    clickRow(presetName);
    return;
  }
  const family: PresetFamilyInfo = PRESET_FAMILIES[preset.family];
  clickRow(family.nameKey ?? family.name);
  const planLabel = `providerPreset.plan.${preset.planKey}`;
  const regionLabel = `providerPreset.region.${preset.regionKey}`;
  const plans = screen.queryByRole("group", {
    name: "providerPreset.planLabel",
  });
  if (plans) {
    clickButtonIn(plans, planLabel);
    clickButtonIn(
      screen.getByRole("group", { name: "providerPreset.regionLabel" }),
      regionLabel,
    );
    return;
  }
  const versions = screen.getByRole("group", {
    name: "providerPreset.versionLabel",
  });
  const label = Array.from(versions.querySelectorAll("button"))
    .map((item) => item.textContent)
    .find((text) => text === planLabel || text === regionLabel);
  clickButtonIn(versions, label ?? planLabel);
}

function fillInputById(container: HTMLElement, id: string, value: string) {
  const input = container.querySelector<HTMLInputElement>(`#${id}`);
  expect(input, `找不到输入框 #${id}`).not.toBeNull();
  expect(input!.disabled, `输入框 #${id} 不应被禁用`).toBe(false);
  fireEvent.change(input!, { target: { value } });
}

/** 列出测试 Key 在行里出现的所有 JSON 路径（字符串内包含也算，例如 TOML 文本） */
function findApiKeyLocations(value: unknown, path = ""): string[] {
  if (typeof value === "string") {
    return value.includes(TEST_API_KEY) ? [path] : [];
  }
  if (Array.isArray(value)) {
    return value.flatMap((item, index) =>
      findApiKeyLocations(item, `${path}[${index}]`),
    );
  }
  if (value && typeof value === "object") {
    return Object.entries(value).flatMap(([key, item]) =>
      findApiKeyLocations(item, path ? `${path}.${key}` : key),
    );
  }
  return [];
}

function toGoldenRow(appId: GoldenAppId, values: ProviderFormValues) {
  // JSON 往返：与 invoke 序列化一致，丢掉值为 undefined 的键
  const serialized = JSON.parse(JSON.stringify(values)) as Record<
    string,
    unknown
  >;
  const { settingsConfig, presetId, ...rest } = serialized;
  const row = {
    ...rest,
    presetId:
      typeof presetId === "string"
        ? presetId.replace(new RegExp(`^${appId}-\\d+$`), `${appId}-<index>`)
        : presetId,
    settingsConfig: JSON.parse(settingsConfig as string) as unknown,
  };
  return {
    apiKeyLocations: findApiKeyLocations(row),
    row,
  };
}

async function submitPresetRow(appId: GoldenAppId, testCase: GoldenCase) {
  const onSubmit = vi.fn();
  const { container } = renderForm(appId, onSubmit);

  clickPreset(appId, testCase.preset);

  for (const [key, value] of Object.entries(testCase.templateValues ?? {})) {
    fillInputById(container, `template-${key}`, value);
  }
  if (testCase.fillApiKey) {
    fillInputById(container, API_KEY_INPUT_ID[appId], TEST_API_KEY);
  }

  fireEvent.click(screen.getByRole("button", { name: SUBMIT_LABEL }));

  await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1));
  expect(toastMocks.error).not.toHaveBeenCalled();
  // 软校验确认框没弹出：说明预设 + Key 已满足提交条件
  expect(screen.queryByText("仍要保存")).not.toBeInTheDocument();

  const values = onSubmit.mock.calls[0][0] as ProviderFormValues;

  // presetId 必须指回被点中的预设（快照里只保留形如 claude-<index> 的格式）
  const presetIndex = Number(values.presetId?.replace(`${appId}-`, ""));
  expect(presetNameAt(appId, presetIndex)).toBe(testCase.preset);

  return toGoldenRow(appId, values);
}

describe("ProviderForm 预设新增行金标", () => {
  beforeEach(() => {
    toastMocks.error.mockReset();
  });

  describe.each<[GoldenAppId, GoldenCase[]]>([
    ["claude", CLAUDE_CASES],
    ["codex", CODEX_CASES],
  ])("%s", (appId, cases) => {
    it.each(cases.map((testCase) => [testCase.preset, testCase] as const))(
      "%s",
      async (_presetName, testCase) => {
        const golden = await submitPresetRow(appId, testCase);
        expect(golden).toMatchSnapshot();
      },
    );
  });
});
