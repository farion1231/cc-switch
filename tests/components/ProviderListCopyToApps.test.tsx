import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ProviderList } from "@/components/providers/ProviderList";
import { server } from "../msw/server";
import {
  resetProviderState,
  setCopyProviderToAppsOutcomes,
} from "../msw/state";

const toastMocks = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  warning: vi.fn(),
}));

const { tMock } = vi.hoisted(() => ({ tMock: vi.fn() }));

vi.mock("sonner", () => ({
  toast: {
    success: toastMocks.success,
    error: toastMocks.error,
    warning: toastMocks.warning,
  },
}));

vi.mock("react-i18next", () => ({
  // useDragSort 从 useTranslation 解构 i18n.language 做本地化排序。
  useTranslation: () => ({ t: tMock, i18n: { language: "zh" } }),
}));

vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({ open, children }: any) =>
    open ? <div data-testid="dialog-root">{children}</div> : null,
  DialogContent: ({ children }: any) => <div>{children}</div>,
  DialogHeader: ({ children }: any) => <div>{children}</div>,
  DialogTitle: ({ children }: any) => <h2>{children}</h2>,
  DialogDescription: ({ children }: any) => <p>{children}</p>,
  DialogFooter: ({ children }: any) => <div>{children}</div>,
}));

// ProviderList 只暴露卡片动作；用按钮替身触发 onCopyToApps，让真实的
// handleCopyToApps 编排逻辑（invoke → 分桶 → 失效缓存 → toast）被执行。
vi.mock("@/components/providers/ProviderCard", () => ({
  ProviderCard: ({ provider, onCopyToApps }: any) => (
    <button type="button" onClick={() => onCopyToApps?.(provider)}>
      copy-from-{provider.id}
    </button>
  ),
}));

const TRANSLATIONS: Record<string, string> = {
  "provider.copyToApps.copyError": "跨应用复制失败",
  "provider.copyToApps.success": "已成功复制到 {{count}} 个应用",
  "provider.copyToApps.successPartial":
    "复制完成：成功 {{copied}} 个，跳过 {{skipped}} 个",
  "provider.copyToApps.resultSummary":
    "复制完成：成功 {{copied}} / 跳过 {{skipped}} / 失败 {{failed}}",
  "provider.copyToApps.reasons.saveFailed": "写入目标应用失败：{{error}}",
};

tMock.mockImplementation((key: string, params?: Record<string, unknown>) => {
  const template =
    TRANSLATIONS[key] ??
    (typeof params?.defaultValue === "string" ? params.defaultValue : key);
  return template.replace(/\{\{(\w+)\}\}/g, (_match, name: string) =>
    params && name in params ? String(params[name]) : `{{${name}}}`,
  );
});

const makeProvider = () => ({
  id: "relay-src",
  name: "Relay",
  settingsConfig: {},
  category: "custom" as const,
  sortIndex: 0,
  createdAt: Date.now(),
});

const renderList = () =>
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <ProviderList
        providers={{ "relay-src": makeProvider() }}
        currentProviderId="relay-src"
        appId="claude"
        onSwitch={vi.fn()}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
        onDuplicate={vi.fn()}
        onOpenWebsite={vi.fn()}
      />
    </QueryClientProvider>,
  );

const openDialogAndPick = async (
  targets: string[],
): Promise<ReturnType<typeof userEvent.setup>> => {
  const user = userEvent.setup();
  await user.click(await screen.findByText("copy-from-relay-src"));
  for (const target of targets) {
    await user.click(screen.getByRole("checkbox", { name: target }));
  }
  await user.click(
    screen.getByRole("button", { name: "provider.copyToApps.copy" }),
  );
  return user;
};

beforeEach(() => {
  resetProviderState();
  toastMocks.success.mockReset();
  toastMocks.error.mockReset();
  toastMocks.warning.mockReset();
});

describe("ProviderList cross-app copy orchestration", () => {
  it("toasts success and closes the dialog when every target is copied", async () => {
    renderList();

    await openDialogAndPick(["Codex"]);

    await waitFor(() =>
      expect(toastMocks.success).toHaveBeenCalledWith("已成功复制到 1 个应用"),
    );
    await waitFor(() =>
      expect(screen.queryByTestId("dialog-root")).not.toBeInTheDocument(),
    );
    expect(toastMocks.error).not.toHaveBeenCalled();
  });

  it("reports mixed results per target and lists skip/fail reasons", async () => {
    setCopyProviderToAppsOutcomes({
      codex: {
        targetApp: "codex",
        status: "copied",
        newProviderId: "relay-src",
        reason: null,
      },
      gemini: {
        targetApp: "gemini",
        status: "skipped",
        newProviderId: null,
        reason: {
          key: "alreadyExists",
          params: { id: "relay-src" },
          fallback: "Gemini already has relay-src",
        },
      },
      openclaw: {
        targetApp: "openclaw",
        status: "failed",
        newProviderId: null,
        reason: {
          key: "saveFailed",
          params: { error: "OpenClaw save failed" },
          fallback: "OpenClaw save failed",
        },
      },
    });
    renderList();

    await openDialogAndPick(["Codex", "Gemini", "OpenClaw"]);

    await waitFor(() => expect(toastMocks.error).toHaveBeenCalledTimes(1));
    const [message, options] = toastMocks.error.mock.calls[0];
    expect(message).toBe("复制完成：成功 1 / 跳过 1 / 失败 1");
    const description = (
      options as { description: { props: { children: string } }[] }
    ).description;
    const lines = description.map((entry) => entry.props.children).join("\n");
    expect(lines).toContain("Gemini: Gemini already has relay-src");
    // saveFailed 键用 {{error}} 插值展示后端具体错误，而不是被通用文案盖掉。
    expect(lines).toContain("OpenClaw: 写入目标应用失败：OpenClaw save failed");

    // 失败项由后端按目标逐个报告，对话框仍正常关闭。
    await waitFor(() =>
      expect(screen.queryByTestId("dialog-root")).not.toBeInTheDocument(),
    );
  });

  it("lists skip reasons even when nothing failed and the dialog still closes", async () => {
    setCopyProviderToAppsOutcomes({
      codex: {
        targetApp: "codex",
        status: "copied",
        newProviderId: "relay-src",
        reason: null,
      },
      gemini: {
        targetApp: "gemini",
        status: "skipped",
        newProviderId: null,
        reason: {
          key: "alreadyExists",
          params: { id: "relay-src" },
          fallback: "Gemini already has relay-src",
        },
      },
    });
    renderList();

    await openDialogAndPick(["Codex", "Gemini"]);

    await waitFor(() => expect(toastMocks.success).toHaveBeenCalledTimes(1));
    const [message, options] = toastMocks.success.mock.calls[0];
    expect(message).toBe("复制完成：成功 1 个，跳过 1 个");
    const description = (
      options as { description: { props: { children: string } }[] }
    ).description;
    const lines = description.map((entry) => entry.props.children).join("\n");
    expect(lines).toContain("Gemini: Gemini already has relay-src");
    expect(toastMocks.warning).not.toHaveBeenCalled();

    await waitFor(() =>
      expect(screen.queryByTestId("dialog-root")).not.toBeInTheDocument(),
    );
  });

  it("warns with skip reasons when every target is skipped", async () => {
    setCopyProviderToAppsOutcomes({
      codex: {
        targetApp: "codex",
        status: "skipped",
        newProviderId: null,
        reason: {
          key: "alreadyExists",
          params: { id: "relay-src" },
          fallback: "Codex already has relay-src",
        },
      },
    });
    renderList();

    await openDialogAndPick(["Codex"]);

    await waitFor(() => expect(toastMocks.warning).toHaveBeenCalledTimes(1));
    const [message, options] = toastMocks.warning.mock.calls[0];
    expect(message).toBe("复制完成：成功 0 个，跳过 1 个");
    const description = (
      options as { description: { props: { children: string } }[] }
    ).description;
    const lines = description.map((entry) => entry.props.children).join("\n");
    expect(lines).toContain("Codex: Codex already has relay-src");
    expect(toastMocks.success).not.toHaveBeenCalled();

    await waitFor(() =>
      expect(screen.queryByTestId("dialog-root")).not.toBeInTheDocument(),
    );
  });

  it("keeps the dialog open and toasts the copy error when invoke rejects", async () => {
    server.use(
      http.post("http://tauri.local/copy_provider_to_apps", () =>
        HttpResponse.json("backend exploded", { status: 500 }),
      ),
    );
    renderList();

    await openDialogAndPick(["Codex"]);

    await waitFor(() => expect(toastMocks.error).toHaveBeenCalledTimes(1));
    expect(toastMocks.error.mock.calls[0][0]).toBe("跨应用复制失败");
    expect(screen.getByTestId("dialog-root")).toBeInTheDocument();
    expect(toastMocks.success).not.toHaveBeenCalled();
  });
});
