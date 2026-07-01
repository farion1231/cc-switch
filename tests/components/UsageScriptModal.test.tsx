import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
import UsageScriptModal from "@/components/UsageScriptModal";
import type { Provider } from "@/types";

// 隔离的重组件：FullScreenPanel + JsonEditor + ConfirmDialog 在测试中替换为最小存根。
vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({
    isOpen,
    children,
    footer,
  }: {
    isOpen: boolean;
    children: ReactNode;
    footer?: ReactNode;
  }) =>
    isOpen ? (
      <div data-testid="usage-script-panel">
        <div>{children}</div>
        <div>{footer}</div>
      </div>
    ) : null,
}));

vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
  }: {
    value: string;
    onChange: (v: string) => void;
  }) => (
    <textarea
      aria-label="mock-json-editor"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: () => null,
}));

vi.mock("@/hooks/useDarkMode", () => ({
  useDarkMode: () => false,
}));

vi.mock("@/lib/api", () => ({
  usageApi: {
    testUsageScript: vi.fn().mockResolvedValue({ success: true, data: [] }),
  },
  settingsApi: {
    save: vi.fn().mockResolvedValue({}),
  },
}));

vi.mock("@/lib/query", () => ({
  useSettingsQuery: () => ({ data: { usageConfirmed: true } }),
}));

vi.mock("@/lib/authBinding", () => ({
  resolveManagedAccountId: () => null,
}));

const makeProvider = (overrides: Partial<Provider> = {}): Provider =>
  ({
    id: "volc-test",
    name: "volc-coding-plan",
    settingsConfig: {
      env: {
        // Agent Plan (`/api/plan`) 不再触发 coding-plan 自动识别（issue #4808
        // revert），这里用仍被识别的 `/api/coding` 来验证 banner 逻辑。
        ANTHROPIC_BASE_URL: "https://ark.cn-beijing.volces.com/api/coding",
        ANTHROPIC_AUTH_TOKEN: "test-key",
      },
    },
    meta: {
      usage_script: {
        enabled: true,
        language: "javascript",
        code: "({ request: { url: '{{baseUrl}}/user/balance', method: 'GET' } })",
        templateType: "general",
        // 模拟用户已经在 modal 里填过通用模板的 apiKey / baseUrl，
        // 但 baseUrl 留空——这正是 issue #4808 的现场。
        apiKey: "stale-script-key",
        baseUrl: "",
      },
    },
    ...overrides,
  }) as unknown as Provider;

function renderModal(provider: Provider) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <UsageScriptModal
        provider={provider}
        appId="claude"
        isOpen
        onClose={() => {}}
        onSave={() => {}}
      />
    </QueryClientProvider>,
  );
}

describe("UsageScriptModal — stale JS template on Coding Plan URL", () => {
  // 升级 banner 中的两个 i18n key：
  //   - usageScript.codingPlanSwitch → "Switch to Coding Plan"
  //   - common.dismiss                → "Dismiss"
  // 测试环境没有初始化 i18next 资源，t() 会原样返回 key。正式运行时（i18n 加载）
  // 会返回本地化字符串。下面的正则同时兼容两种情况。
  const SWITCH_TEXT = /Switch to Coding Plan|usageScript\.codingPlanSwitch/;
  const DISMISS_TEXT = /Dismiss|common\.dismiss/;

  it("renders the upgrade banner when saved templateType is a JS template and baseUrl matches a coding-plan vendor", () => {
    renderModal(makeProvider());
    expect(screen.getByText(SWITCH_TEXT)).toBeInTheDocument();
  });

  it("does not render the banner when the saved template is already token_plan (or none)", () => {
    renderModal(
      makeProvider({
        meta: {
          usage_script: {
            enabled: true,
            language: "javascript",
            code: "",
            templateType: "token_plan",
            codingPlanProvider: "volcengine",
          },
        },
      }),
    );
    expect(screen.queryByText(SWITCH_TEXT)).not.toBeInTheDocument();
  });

  it("does not render the banner when the baseUrl is not a coding-plan vendor", () => {
    renderModal(
      makeProvider({
        settingsConfig: {
          env: { ANTHROPIC_BASE_URL: "https://api.openai.com/v1" },
        },
      }),
    );
    expect(screen.queryByText(SWITCH_TEXT)).not.toBeInTheDocument();
  });

  it("Dismiss hides the banner without changing provider state", () => {
    renderModal(makeProvider());
    const dismiss = screen.getByRole("button", { name: DISMISS_TEXT });
    fireEvent.click(dismiss);
    expect(screen.queryByText(SWITCH_TEXT)).not.toBeInTheDocument();
  });
});
