import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProviderForm } from "@/components/providers/forms/ProviderForm";
import type { ProviderMeta } from "@/types";
import type { AppMode } from "@/types/proxy";
import { createTestQueryClient } from "../utils/testQueryClient";

// 完整表单按应用实际模式提示后果：直连时非原生格式发不出请求、兜底模型就是主模型。
const modeState = vi.hoisted(() => ({
  mode: "direct" as AppMode,
  directProviderId: null as string | null,
}));

vi.mock("@/lib/query/proxy", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/query/proxy")>();
  return {
    ...actual,
    useAppMode: () => ({
      data: {
        mode: modeState.mode,
        attached: false,
        routeProviderId: null,
        directProviderId: modeState.directProviderId,
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

const IN_USE =
  "这张卡正在直连使用：改成这种格式后保存，请求会失败。请先切到路由或聚合。";
const SWITCH = "直连模式下用不了这种格式，切到路由或聚合后才能用这张卡。";
const FALLBACK_DIRECT =
  /^写进 ANTHROPIC_MODEL，是 Claude Code 启动时用的主模型/;
const FALLBACK_ROUTE = /^没落到 Sonnet、Opus、Fable、Haiku 任一档的请求/;
const HAIKU_MISSING = /^Haiku 一档没填/;
const FALLBACK_MISSING = /^没填：没落到任何一档的请求/;

function renderClaude(
  meta?: ProviderMeta,
  baseUrl = "https://relay.example",
  extraEnv: Record<string, string> = {},
) {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ProviderForm
        appId="claude"
        providerId="relay"
        submitLabel="save-provider"
        onSubmit={vi.fn()}
        onCancel={vi.fn()}
        initialData={{
          name: "Relay",
          category: "third_party",
          settingsConfig: {
            env: {
              ANTHROPIC_BASE_URL: baseUrl,
              ANTHROPIC_AUTH_TOKEN: "sk-test",
              ANTHROPIC_MODEL: "m-a",
              ...extraEnv,
            },
          },
          meta,
        }}
      />
    </QueryClientProvider>,
  );
}

/** 有已配置的高级值时高级区会自动展开，只在折叠时点开，免得点一下反而收起。 */
function expandClaudeAdvanced() {
  const toggle = screen.getByRole("button", {
    name: "providerForm.advancedOptionsToggle",
  });
  if (toggle.getAttribute("aria-expanded") !== "true") {
    fireEvent.click(toggle);
  }
  expect(toggle).toHaveAttribute("aria-expanded", "true");
}

function renderCodex(meta?: ProviderMeta) {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ProviderForm
        appId="codex"
        providerId="relay"
        submitLabel="save-provider"
        onSubmit={vi.fn()}
        onCancel={vi.fn()}
        initialData={{
          name: "Relay",
          category: "third_party",
          settingsConfig: {
            auth: {},
            config:
              'model_provider = "custom"\nmodel = "m-1"\n' +
              '\n[model_providers.custom]\nname = "custom"\nbase_url = "https://relay.example/v1"\nexperimental_bearer_token = "sk-test"\n',
          },
          meta,
        }}
      />
    </QueryClientProvider>,
  );
}

describe("完整表单按实际模式提示（Claude Code）", () => {
  beforeEach(() => {
    modeState.mode = "direct";
    modeState.directProviderId = null;
  });

  it("正在直连用的卡选了非原生格式：提示保存后请求会失败", () => {
    modeState.directProviderId = "relay";
    renderClaude({ apiFormat: "openai_chat" });

    expect(screen.getByText(IN_USE)).toBeInTheDocument();
    expect(screen.getByText(FALLBACK_DIRECT)).toBeInTheDocument();
    expect(screen.queryByText(FALLBACK_ROUTE)).toBeNull();
  });

  it("直连下别的卡选了非原生格式：提示要先切到路由或聚合", () => {
    renderClaude({ apiFormat: "openai_chat" });

    expect(screen.getByText(SWITCH)).toBeInTheDocument();
    expect(screen.queryByText(IN_USE)).toBeNull();
  });

  it("路由模式下不提示格式后果，兜底模型按路由说明", () => {
    modeState.mode = "route";
    renderClaude({ apiFormat: "openai_chat" });

    expect(screen.queryByText(IN_USE)).toBeNull();
    expect(screen.queryByText(SWITCH)).toBeNull();
    expect(screen.getByText(FALLBACK_ROUTE)).toBeInTheDocument();
  });

  it("直连下第三方端点清空 Haiku 档：提示后台任务会用原始模型名", () => {
    // Haiku 档缺省时表单从 ANTHROPIC_MODEL 回填，只有显式清空才会空着。
    renderClaude(undefined, "https://relay.example", {
      ANTHROPIC_DEFAULT_HAIKU_MODEL: "",
    });
    expandClaudeAdvanced();
    expect(screen.getByText(HAIKU_MISSING)).toBeInTheDocument();
  });

  it("直连下填了 Haiku 档就不提示", () => {
    renderClaude(undefined, "https://relay.example", {
      ANTHROPIC_DEFAULT_HAIKU_MODEL: "m-h",
    });
    expandClaudeAdvanced();
    expect(screen.queryByText(HAIKU_MISSING)).toBeNull();
  });

  it("路由下兜底模型空着才提示，Haiku 档不看", () => {
    modeState.mode = "route";
    renderClaude(undefined, "https://relay.example", { ANTHROPIC_MODEL: "" });
    expandClaudeAdvanced();
    expect(screen.getByText(FALLBACK_MISSING)).toBeInTheDocument();
    expect(screen.queryByText(HAIKU_MISSING)).toBeNull();
  });

  it("普通卡照常显示地址、格式、认证字段、UA（下面两个用例的对照）", () => {
    modeState.mode = "route";
    renderClaude();
    expandClaudeAdvanced();

    expect(document.getElementById("baseUrl")).not.toBeNull();
    expect(document.getElementById("apiFormat")).not.toBeNull();
    expect(screen.getByText("认证字段")).toBeInTheDocument();
    expect(document.getElementById("claude-custom-user-agent")).not.toBeNull();
  });

  it("字段说明收进「?」，页面上只留后果提示", () => {
    modeState.mode = "route";
    renderClaude();
    expandClaudeAdvanced();

    // HelpTip 给屏幕阅读器常驻一份 sr-only 副本，只看页面上露出来的。
    const shownOnPage = (text: string | RegExp) =>
      screen.queryAllByText(text).filter((el) => !el.closest(".sr-only"));

    expect(
      screen.getByRole("button", { name: "上游格式" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "认证字段" }),
    ).toBeInTheDocument();
    expect(shownOnPage(/^直连时是写进 settings\.json/)).toHaveLength(0);
    expect(shownOnPage("providerForm.modelMappingHint")).toHaveLength(0);
    expect(shownOnPage(/^看供应商原生支持哪种协议/)).toHaveLength(0);
    expect(
      screen.getByText("以下两项只在路由、聚合模式下生效。"),
    ).toBeInTheDocument();
  });

  it("OpenAI 格式不显示认证字段（路由一律发 Bearer）", () => {
    modeState.mode = "route";
    renderClaude({ apiFormat: "openai_chat" });
    expandClaudeAdvanced();

    expect(document.getElementById("apiFormat")).not.toBeNull();
    expect(screen.queryByText("认证字段")).toBeNull();
  });

  it("Codex 托管账号卡不显示后端固定的地址、格式和认证字段", () => {
    modeState.mode = "route";
    renderClaude({ providerType: "codex_oauth" });
    expandClaudeAdvanced();

    expect(document.getElementById("baseUrl")).toBeNull();
    expect(document.getElementById("apiFormat")).toBeNull();
    expect(screen.queryByText("认证字段")).toBeNull();
  });

  it("Copilot 卡不显示会被转发跳过的 UA 和请求覆盖", () => {
    modeState.mode = "route";
    renderClaude(undefined, "https://api.githubcopilot.com");
    expandClaudeAdvanced();

    expect(document.getElementById("claude-custom-user-agent")).toBeNull();
  });
});

describe("完整表单按实际模式提示（Codex）", () => {
  beforeEach(() => {
    modeState.mode = "direct";
    modeState.directProviderId = "relay";
  });

  it("正在直连用的卡选了 Chat：格式下提示保存后请求会失败", () => {
    renderCodex({ apiFormat: "openai_chat" });
    fireEvent.click(screen.getByRole("button", { name: /高级选项/ }));

    expect(screen.getByText(IN_USE)).toBeInTheDocument();
  });

  it("Responses 原生格式不提示", () => {
    renderCodex({ apiFormat: "openai_responses" });
    fireEvent.click(screen.getByRole("button", { name: /高级选项/ }));

    expect(document.getElementById("codex-upstream-format")).not.toBeNull();
    expect(screen.queryByText(IN_USE)).toBeNull();
  });
});

describe("完整 URL 开关在高级选项里", () => {
  beforeEach(() => {
    modeState.mode = "route";
    modeState.directProviderId = null;
  });

  const fullUrlSwitch = () =>
    screen.queryByRole("switch", { name: "完整 URL" });

  it("Claude：高级区折叠时看不到，展开后在里面", () => {
    renderClaude(undefined, "https://relay.example", { ANTHROPIC_MODEL: "" });

    expect(fullUrlSwitch()).toBeNull();
    expandClaudeAdvanced();
    expect(fullUrlSwitch()).not.toBeChecked();
  });

  it("Claude：已开着的卡自动展开高级区", () => {
    renderClaude({ isFullUrl: true }, "https://relay.example", {
      ANTHROPIC_MODEL: "",
    });

    expect(fullUrlSwitch()).toBeChecked();
  });

  it("Codex：平时不显示，已开着的卡自动展开高级区", () => {
    const { unmount } = renderCodex();
    expect(fullUrlSwitch()).toBeNull();
    unmount();

    renderCodex({ isFullUrl: true });
    expect(fullUrlSwitch()).toBeChecked();
  });
});
