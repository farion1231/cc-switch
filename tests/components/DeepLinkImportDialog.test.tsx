import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  QueryClient,
  QueryClientProvider,
  useQuery,
} from "@tanstack/react-query";
import { Profiler } from "react";
import { DeepLinkImportDialog } from "@/components/DeepLinkImportDialog";
import { http, HttpResponse } from "msw";
import i18n from "i18next";
import { emitTauriEvent } from "../msw/tauriMocks";
import { server } from "../msw/server";
import { peekPendingDeepLink, setPendingDeepLink } from "../msw/state";
import { pngDataUri } from "../fixtures/customIcon";
import { toast } from "@/lib/toast";

// jsdom 没有 canvas，重新编码这一步换成可控的假实现
const { normalizeIconDataUri } = vi.hoisted(() => ({
  normalizeIconDataUri: vi.fn<(dataUri: string) => Promise<string>>(),
}));
vi.mock("@/utils/customIcon", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/utils/customIcon")>()),
  normalizeIconDataUri,
}));

// 模拟后端：先暂存链接，再发 deeplink-pending 通知前端来取
const deliverDeepLink = (request: Record<string, unknown>) => {
  setPendingDeepLink({ kind: "import", request });
  emitTauriEvent("deeplink-pending", null);
};

vi.mock("@/components/ui/dialog", () => ({
  // 内容始终渲染，开关状态记在属性上，供断言确认框是否仍打开
  Dialog: ({
    children,
    open,
  }: {
    children: React.ReactNode;
    open?: boolean;
  }) => (
    <div data-testid="deeplink-dialog" data-open={String(!!open)}>
      {children}
    </div>
  ),
  DialogContent: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogHeader: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogTitle: ({ children }: { children: React.ReactNode }) => (
    <h1>{children}</h1>
  ),
  DialogDescription: ({ children }: { children: React.ReactNode }) => (
    <p>{children}</p>
  ),
  DialogFooter: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
}));

const Wrapper = ({ children }: { children: React.ReactNode }) => (
  <QueryClientProvider client={new QueryClient()}>
    {children}
  </QueryClientProvider>
);

describe("DeepLinkImportDialog", () => {
  it.each([
    { app: "opencode", enabled: true, fails: false, refreshes: true },
    { app: "opencode", enabled: false, fails: false, refreshes: true },
    { app: "opencode", enabled: true, fails: true, refreshes: false },
    { app: "claude", enabled: true, fails: false, refreshes: false },
  ])(
    "refreshes OpenCode live data only after its successful import: %j",
    async ({ app, enabled, fails, refreshes }) => {
      const success = vi.spyOn(toast, "success");
      const failure = vi.spyOn(toast, "error");
      const client = new QueryClient({
        defaultOptions: { queries: { staleTime: Infinity, retry: false } },
      });
      let liveIds: string[] = [];
      let models: string[] = [];
      const readLiveIds = vi.fn(async () => liveIds);
      const readModels = vi.fn(async () => models);
      const importProvider = vi.fn(async () => {
        if (fails)
          return HttpResponse.json(
            { message: "Import failed" },
            { status: 500 },
          );
        if (app === "opencode" && enabled) {
          liveIds = ["imported-provider"];
          models = ["imported-provider/model"];
        }
        return HttpResponse.json({ type: "provider", id: "imported-provider" });
      });
      server.use(
        http.post(
          "http://tauri.local/import_from_deeplink_unified",
          importProvider,
        ),
      );
      function LiveState() {
        const ids = useQuery({
          queryKey: ["opencodeLiveProviderIds"],
          queryFn: readLiveIds,
        });
        const runtimeModels = useQuery({
          queryKey: ["opencode", "runtime-models"],
          queryFn: readModels,
        });
        return (
          <>
            <output data-testid="live-ids">{JSON.stringify(ids.data)}</output>
            <output data-testid="runtime-models">
              {JSON.stringify(runtimeModels.data)}
            </output>
          </>
        );
      }
      render(
        <QueryClientProvider client={client}>
          <LiveState />
          <DeepLinkImportDialog />
        </QueryClientProvider>,
      );
      await waitFor(() =>
        expect(screen.getByTestId("live-ids")).toHaveTextContent("[]"),
      );
      await waitFor(() =>
        expect(screen.getByTestId("runtime-models")).toHaveTextContent("[]"),
      );
      act(() =>
        deliverDeepLink({
          version: "v1",
          resource: "provider",
          app,
          enabled,
          name: "Imported Provider",
          endpoint: "https://example.com/v1",
          apiKey: "public-test-key",
        }),
      );
      const user = userEvent.setup();
      await user.click(
        await screen.findByRole("button", { name: "deeplink.import" }),
      );
      await waitFor(() => expect(importProvider).toHaveBeenCalledTimes(1));
      if (fails) {
        await waitFor(() => expect(failure).toHaveBeenCalled());
      } else {
        await waitFor(() => expect(success).toHaveBeenCalled());
      }
      expect(readLiveIds).toHaveBeenCalledTimes(refreshes ? 2 : 1);
      expect(readModels).toHaveBeenCalledTimes(refreshes ? 2 : 1);
      const shouldBeLive = refreshes && enabled;
      await waitFor(() =>
        expect(screen.getByTestId("live-ids")).toHaveTextContent(
          shouldBeLive ? '["imported-provider"]' : "[]",
        ),
      );
      await waitFor(() =>
        expect(screen.getByTestId("runtime-models")).toHaveTextContent(
          shouldBeLive ? '["imported-provider/model"]' : "[]",
        ),
      );
      client.clear();
      success.mockRestore();
      failure.mockRestore();
    },
  );

  it("renders masked usage access token and user id for provider imports", async () => {
    render(<DeepLinkImportDialog />, { wrapper: Wrapper });

    act(() => {
      deliverDeepLink({
        version: "v1",
        resource: "provider",
        app: "claude",
        name: "Test Provider",
        homepage: "https://example.com",
        endpoint: "https://api.example.com",
        apiKey: "sk-provider-key",
        usageEnabled: true,
        usageScript: btoa("console.log('usage');"),
        usageApiKey: "sk-usage-key",
        usageBaseUrl: "https://usage.example.com",
        usageAccessToken: "pat-secret-token",
        usageUserId: "user-12345",
        usageAutoInterval: 60,
      });
    });

    await waitFor(() => {
      expect(screen.getByText("用量访问令牌")).toBeInTheDocument();
    });

    expect(screen.getByText("用量用户 ID")).toBeInTheDocument();
    expect(screen.getByText("user-12345")).toBeInTheDocument();
    // Masked: first 4 chars + 12 stars
    expect(screen.getByText("pat-************")).toBeInTheDocument();
  });

  it("shows usage credentials even when the deeplink carries no usageScript", async () => {
    // 后端 build_provider_meta 在任一 usage 字段存在时即持久化（含 access_token
    // 与 user_id）。若对话框只在 usageScript 存在时开门，这条链接会把凭据静默
    // 写进供应商配置。撤销门槛 widening（恢复只按 usageScript 开门）本测试即失败。
    render(<DeepLinkImportDialog />, { wrapper: Wrapper });

    act(() => {
      deliverDeepLink({
        version: "v1",
        resource: "provider",
        app: "claude",
        name: "Token Only Provider",
        homepage: "https://example.com",
        endpoint: "https://api.example.com",
        apiKey: "sk-provider-key",
        usageAccessToken: "pat-secret-token",
        usageUserId: "user-12345",
      });
    });

    await waitFor(() => {
      expect(screen.getByText("用量访问令牌")).toBeInTheDocument();
    });

    expect(screen.getByText("pat-************")).toBeInTheDocument();
    expect(screen.getByText("用量用户 ID")).toBeInTheDocument();
    expect(screen.getByText("user-12345")).toBeInTheDocument();
    // 没有脚本就不应渲染脚本执行警告与脚本代码区
    expect(
      screen.queryByText(
        "这是一段 JavaScript 代码，启用后会在查询用量时执行。请确认来源可信后再导入。",
      ),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("脚本代码")).not.toBeInTheDocument();
  });

  // 徽章要和后端 `usage_enabled.unwrap_or(!code.is_empty())` 一致：
  // 链接写了 usageEnabled 就照办，没写时带脚本即启用
  it.each([
    { usageEnabled: undefined, expected: "已启用" },
    { usageEnabled: true, expected: "已启用" },
    { usageEnabled: false, expected: "未启用" },
  ])(
    "shows the usage badge as $expected when usageEnabled is $usageEnabled and a script is attached",
    async ({ usageEnabled, expected }) => {
      render(<DeepLinkImportDialog />, { wrapper: Wrapper });

      act(() => {
        deliverDeepLink({
          version: "v1",
          resource: "provider",
          app: "claude",
          name: "Usage Provider",
          homepage: "https://example.com",
          endpoint: "https://api.example.com",
          apiKey: "sk-provider-key",
          usageScript: btoa("console.log('usage');"),
          ...(usageEnabled === undefined ? {} : { usageEnabled }),
        });
      });

      await waitFor(() => {
        expect(screen.getByText(expected)).toBeInTheDocument();
      });
    },
  );

  // 套餐地址、不带脚本：后端按套餐额度查询并默认启用，徽章与后端一致
  it.each([
    {
      app: "codex",
      endpoint: "https://api.kimi.com/coding/v1",
      usageEnabled: undefined,
      expected: "已启用",
    },
    {
      app: "codex",
      endpoint: "https://api.kimi.com/coding/v1",
      usageEnabled: false,
      expected: "未启用",
    },
  ])(
    "shows the plan quota badge as $expected for $app with usageEnabled $usageEnabled",
    async ({ app, endpoint, usageEnabled, expected }) => {
      render(<DeepLinkImportDialog />, { wrapper: Wrapper });

      act(() => {
        deliverDeepLink({
          version: "v1",
          resource: "provider",
          app,
          name: "Plan Provider",
          homepage: "https://example.com",
          endpoint,
          apiKey: "sk-provider-key",
          ...(usageEnabled === undefined ? {} : { usageEnabled }),
        });
      });

      await waitFor(() => {
        expect(screen.getByText(expected)).toBeInTheDocument();
      });
      expect(screen.getByText("套餐额度：Kimi For Coding")).toBeInTheDocument();
    },
  );

  it("does not show a usage row for ZenMux outside Claude", async () => {
    render(<DeepLinkImportDialog />, { wrapper: Wrapper });

    act(() => {
      deliverDeepLink({
        version: "v1",
        resource: "provider",
        app: "codex",
        name: "ZenMux Provider",
        homepage: "https://example.com",
        endpoint: "https://zenmux.ai/api/v1",
        apiKey: "sk-provider-key",
      });
    });

    await waitFor(() => {
      expect(screen.getByText("ZenMux Provider")).toBeInTheDocument();
    });
    expect(screen.queryByText("用量查询")).not.toBeInTheDocument();
  });

  it("shows a link that arrived before the dialog mounted (cold start)", async () => {
    // 冷启动时链接早于前端监听到达，后端已暂存；组件挂载后必须主动取一次
    setPendingDeepLink({
      kind: "import",
      request: {
        version: "v1",
        resource: "provider",
        app: "claude",
        name: "Cold Start Provider",
        homepage: "https://example.com",
        endpoint: "https://api.example.com",
        apiKey: "sk-provider-key",
      },
    });

    render(<DeepLinkImportDialog />, { wrapper: Wrapper });

    await waitFor(() => {
      expect(screen.getByText("Cold Start Provider")).toBeInTheDocument();
    });
  });

  it("warns that enabled=true switches to the provider right after import", async () => {
    render(<DeepLinkImportDialog />, { wrapper: Wrapper });

    act(() => {
      deliverDeepLink({
        version: "v1",
        resource: "provider",
        app: "claude",
        name: "Switching Provider",
        homepage: "https://example.com",
        endpoint: "https://api.example.com",
        apiKey: "sk-provider-key",
        enabled: true,
      });
    });

    await waitFor(() => {
      expect(
        screen.getByText("deeplink.providerEnabledWarning"),
      ).toBeInTheDocument();
    });
  });

  it("does not show the switch warning when enabled is absent", async () => {
    render(<DeepLinkImportDialog />, { wrapper: Wrapper });

    act(() => {
      deliverDeepLink({
        version: "v1",
        resource: "provider",
        app: "claude",
        name: "Add Only Provider",
        homepage: "https://example.com",
        endpoint: "https://api.example.com",
        apiKey: "sk-provider-key",
      });
    });

    await waitFor(() => {
      expect(screen.getByText("Add Only Provider")).toBeInTheDocument();
    });
    expect(
      screen.queryByText("deeplink.providerEnabledWarning"),
    ).not.toBeInTheDocument();
  });

  describe("the activate-after-import switch", () => {
    // 记下导入请求里的 enabled：开关的值必须覆盖链接里的 enabled
    const captureImport = () => {
      const sent: Array<Record<string, unknown>> = [];
      server.use(
        http.post(
          "http://tauri.local/import_from_deeplink_unified",
          async ({ request }) => {
            const body = (await request.json()) as {
              request: Record<string, unknown>;
            };
            sent.push(body.request);
            return HttpResponse.json(
              body.request.resource === "prompt"
                ? { type: "prompt", id: "p1" }
                : { type: "provider", id: "p1" },
            );
          },
        ),
      );
      return sent;
    };

    const providerLink = (extra: Record<string, unknown>) => ({
      version: "v1",
      resource: "provider",
      name: "Toggle Provider",
      homepage: "https://example.com",
      endpoint: "https://api.example.com",
      apiKey: "sk-provider-key",
      ...extra,
    });

    it.each([
      {
        resource: "provider",
        app: "opencode",
      },
      {
        resource: "prompt",
        app: "claude",
        content: btoa("# review"),
        enabled: true,
      },
    ])(
      "shows the correct activation state on the first $resource render",
      async (link) => {
        const renderedStates: Array<string | null> = [];
        render(
          <Profiler
            id="deeplink-activation"
            onRender={() => {
              // Observe every committed render, before passive effects can fix a stale value.
              const toggle = screen.queryByRole("switch");
              if (toggle)
                renderedStates.push(toggle.getAttribute("aria-checked"));
            }}
          >
            <DeepLinkImportDialog />
          </Profiler>,
          { wrapper: Wrapper },
        );
        act(() => deliverDeepLink(providerLink(link)));

        await screen.findByRole("switch");
        expect(renderedStates.length).toBeGreaterThan(0);
        expect(new Set(renderedStates)).toEqual(new Set(["true"]));
      },
    );

    it("starts from the link's enabled and lets the user turn switching off", async () => {
      const sent = captureImport();
      const user = userEvent.setup();
      render(<DeepLinkImportDialog />, { wrapper: Wrapper });
      act(() =>
        deliverDeepLink(providerLink({ app: "claude", enabled: true })),
      );

      const toggle = await screen.findByRole("switch");
      expect(toggle).toBeChecked();
      expect(
        screen.getByText("deeplink.providerEnabledWarning"),
      ).toBeInTheDocument();

      await user.click(toggle);
      expect(toggle).not.toBeChecked();
      expect(
        screen.queryByText("deeplink.providerEnabledWarning"),
      ).not.toBeInTheDocument();

      await user.click(screen.getByRole("button", { name: "deeplink.import" }));
      await waitFor(() => expect(sent).toHaveLength(1));
      expect(sent[0].enabled).toBe(false);
    });

    it("lets the user opt into switching when the link does not ask for it", async () => {
      const sent = captureImport();
      const user = userEvent.setup();
      render(<DeepLinkImportDialog />, { wrapper: Wrapper });
      act(() => deliverDeepLink(providerLink({ app: "codex" })));

      const toggle = await screen.findByRole("switch");
      expect(toggle).not.toBeChecked();
      expect(
        screen.getByText("deeplink.switchAfterImport"),
      ).toBeInTheDocument();

      await user.click(toggle);
      expect(
        screen.getByText("deeplink.providerEnabledWarning"),
      ).toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "deeplink.import" }));
      await waitFor(() => expect(sent).toHaveLength(1));
      expect(sent[0].enabled).toBe(true);
    });

    it("only imports Antigravity providers, even when the link asks to enable", async () => {
      const sent = captureImport();
      const user = userEvent.setup();
      render(<DeepLinkImportDialog />, { wrapper: Wrapper });
      act(() => deliverDeepLink(providerLink({ app: "agy", enabled: true })));

      const importButton = await screen.findByRole("button", {
        name: "deeplink.import",
      });
      // Antigravity 的卡没有「启用」，不给开关也不提示改发请求
      expect(screen.queryByRole("switch")).not.toBeInTheDocument();
      expect(
        screen.queryByText("deeplink.providerEnabledWarning"),
      ).not.toBeInTheDocument();
      await user.click(importButton);
      await waitFor(() => expect(sent).toHaveLength(1));
      expect(sent[0].enabled).toBe(false);
    });

    it("adds additive-app providers to the app config unless the link opts out", async () => {
      render(<DeepLinkImportDialog />, { wrapper: Wrapper });
      act(() => deliverDeepLink(providerLink({ app: "opencode" })));

      const toggle = await screen.findByRole("switch");
      expect(toggle).toBeChecked();
      expect(
        screen.getByText("deeplink.addToAppAfterImport"),
      ).toBeInTheDocument();
      // 累加式应用没有「切换」，不提示改发请求
      expect(
        screen.queryByText("deeplink.providerEnabledWarning"),
      ).not.toBeInTheDocument();
    });

    it("lets the user decide whether an imported prompt is enabled", async () => {
      const sent = captureImport();
      const user = userEvent.setup();
      render(<DeepLinkImportDialog />, { wrapper: Wrapper });
      act(() =>
        deliverDeepLink({
          version: "v1",
          resource: "prompt",
          app: "claude",
          name: "Review Prompt",
          content: btoa("# review"),
          enabled: true,
        }),
      );

      const toggle = await screen.findByRole("switch");
      expect(toggle).toBeChecked();
      expect(
        screen.getByText("deeplink.prompt.enabledWarning"),
      ).toBeInTheDocument();

      await user.click(toggle);
      await user.click(screen.getByRole("button", { name: "deeplink.import" }));
      await waitFor(() => expect(sent).toHaveLength(1));
      expect(sent[0].enabled).toBe(false);
    });
  });

  describe("the link's iconUrl", () => {
    const DOWNLOADED = "data:image/svg+xml;base64,PHN2Zy8+";
    const NORMALIZED = pngDataUri(96, 96);

    const iconLink = (extra: Record<string, unknown> = {}) => ({
      version: "v1",
      resource: "provider",
      app: "claude",
      name: "Icon Provider",
      endpoint: "https://api.example.com",
      apiKey: "sk-provider-key",
      iconUrl: "https://cdn.example.com/logo.svg",
      ...extra,
    });

    // 记下下载请求和导入请求；download 决定后端返回什么
    const mockBackend = (download: () => Promise<string | null>) => {
      const fetched: string[] = [];
      const sent: Array<Record<string, unknown>> = [];
      server.use(
        http.post(
          "http://tauri.local/fetch_deeplink_icon",
          async ({ request }) => {
            const { url } = (await request.json()) as { url: string };
            fetched.push(url);
            return HttpResponse.json(await download());
          },
        ),
        http.post(
          "http://tauri.local/import_from_deeplink_unified",
          async ({ request }) => {
            const body = (await request.json()) as {
              request: Record<string, unknown>;
            };
            sent.push(body.request);
            return HttpResponse.json({ type: "provider", id: "p1" });
          },
        ),
      );
      return { fetched, sent };
    };

    afterEach(() => {
      normalizeIconDataUri.mockReset();
    });

    it("downloads the icon on open, previews it and imports it without asking", async () => {
      normalizeIconDataUri.mockResolvedValue(NORMALIZED);
      const { fetched, sent } = mockBackend(async () => DOWNLOADED);
      const user = userEvent.setup();
      render(<DeepLinkImportDialog />, { wrapper: Wrapper });
      act(() => deliverDeepLink(iconLink({ icon: "deepseek" })));

      const preview = await screen.findByAltText("Icon Provider");
      expect(preview).toHaveAttribute("src", NORMALIZED);
      expect(fetched).toEqual(["https://cdn.example.com/logo.svg"]);
      expect(normalizeIconDataUri).toHaveBeenCalledWith(DOWNLOADED);

      await user.click(screen.getByRole("button", { name: "deeplink.import" }));
      await waitFor(() => expect(sent).toHaveLength(1));
      expect(sent[0].icon).toBe(NORMALIZED);
      expect(sent[0]).not.toHaveProperty("iconUrl");
    });

    it("falls back to the built-in icon when the download fails", async () => {
      const { sent } = mockBackend(async () => null);
      const user = userEvent.setup();
      render(<DeepLinkImportDialog />, { wrapper: Wrapper });
      act(() => deliverDeepLink(iconLink({ icon: "deepseek" })));

      await user.click(
        await screen.findByRole("button", { name: "deeplink.import" }),
      );
      await waitFor(() => expect(sent).toHaveLength(1));
      expect(sent[0].icon).toBe("deepseek");
      expect(normalizeIconDataUri).not.toHaveBeenCalled();
    });

    it("imports without an icon when re-encoding fails and there is no fallback", async () => {
      normalizeIconDataUri.mockRejectedValue(new Error("tainted canvas"));
      const { sent } = mockBackend(async () => DOWNLOADED);
      const user = userEvent.setup();
      render(<DeepLinkImportDialog />, { wrapper: Wrapper });
      act(() => deliverDeepLink(iconLink()));

      // 首字母占位
      expect(await screen.findByText("IP")).toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "deeplink.import" }));
      await waitFor(() => expect(sent).toHaveLength(1));
      expect(sent[0].icon).toBeUndefined();
    });

    it("waits for a download still in flight when the user imports", async () => {
      normalizeIconDataUri.mockResolvedValue(NORMALIZED);
      let finishDownload!: (value: string) => void;
      const download = new Promise<string>((resolve) => {
        finishDownload = resolve;
      });
      const { fetched, sent } = mockBackend(() => download);
      const user = userEvent.setup();
      render(<DeepLinkImportDialog />, { wrapper: Wrapper });
      act(() => deliverDeepLink(iconLink()));

      await waitFor(() => expect(fetched).toHaveLength(1));
      await user.click(screen.getByRole("button", { name: "deeplink.import" }));
      // 下载没完成之前不提交
      await new Promise((resolve) => setTimeout(resolve, 50));
      expect(sent).toHaveLength(0);

      finishDownload(DOWNLOADED);
      await waitFor(() => expect(sent).toHaveLength(1));
      expect(sent[0].icon).toBe(NORMALIZED);
    });

    it("does not download for links without iconUrl", async () => {
      const { fetched } = mockBackend(async () => DOWNLOADED);
      render(<DeepLinkImportDialog />, { wrapper: Wrapper });
      act(() =>
        deliverDeepLink(iconLink({ iconUrl: undefined, icon: "deepseek" })),
      );

      await screen.findByRole("button", { name: "deeplink.import" });
      expect(fetched).toHaveLength(0);
    });
  });

  describe("when processing is interrupted after the link was read", () => {
    const coldStartLink = {
      kind: "import",
      request: {
        version: "v1",
        resource: "provider",
        app: "claude",
        name: "Interrupted Provider",
        homepage: "https://example.com",
        endpoint: "https://api.example.com",
        apiKey: "sk-provider-key",
      },
    };

    // 让 peek 的响应停在半路：返回 hit（请求已到达）与 release（放行响应）
    const holdPeek = () => {
      let release!: () => void;
      const gate = new Promise<void>((resolve) => (release = resolve));
      let markHit!: () => void;
      const hit = new Promise<void>((resolve) => (markHit = resolve));
      server.use(
        http.post("http://tauri.local/peek_pending_deeplink", async () => {
          const entry = peekPendingDeepLink();
          markHit();
          await gate;
          return HttpResponse.json(entry);
        }),
      );
      return { hit, release };
    };

    afterEach(async () => {
      await act(async () => {
        await i18n.changeLanguage("zh");
      });
    });

    it("still shows the link when the language changes mid-read", async () => {
      // 启动时按后端设置校正语言会换掉 t；effect 若随 t 重建，读到一半的链接就被丢下
      setPendingDeepLink(coldStartLink);
      const { hit, release } = holdPeek();

      render(<DeepLinkImportDialog />, { wrapper: Wrapper });
      await hit;
      await act(async () => {
        await i18n.changeLanguage("en");
      });
      release();

      await waitFor(() => {
        expect(screen.getByText("Interrupted Provider")).toBeInTheDocument();
      });
      await waitFor(() => expect(peekPendingDeepLink()).toBeNull());
    });

    it("keeps the link for the next mount when unmounted mid-read", async () => {
      // 窗口销毁重建：旧实例读到了但没显示，链接必须留在后端给新实例
      setPendingDeepLink(coldStartLink);
      const { hit, release } = holdPeek();

      const first = render(<DeepLinkImportDialog />, { wrapper: Wrapper });
      await hit;
      first.unmount();
      release();
      server.resetHandlers();

      expect(peekPendingDeepLink()).not.toBeNull();
      render(<DeepLinkImportDialog />, { wrapper: Wrapper });

      await waitFor(() => {
        expect(screen.getByText("Interrupted Provider")).toBeInTheDocument();
      });
    });

    it("does not let a late read of an older link replace a newer one", async () => {
      // A 的读取响应迟到；期间 B 到达。若读取可以并发，B 先显示并被清除，
      // A 随后返回把 B 盖掉，B 从界面消失、后端也取不回来了
      const providerLink = (name: string) => ({
        ...coldStartLink,
        request: { ...coldStartLink.request, name },
      });
      setPendingDeepLink(providerLink("Older Provider"));

      let release!: () => void;
      const gate = new Promise<void>((resolve) => (release = resolve));
      let markHit!: () => void;
      const hit = new Promise<void>((resolve) => (markHit = resolve));
      let calls = 0;
      server.use(
        http.post("http://tauri.local/peek_pending_deeplink", async () => {
          const entry = peekPendingDeepLink();
          if (++calls === 1) {
            markHit();
            await gate;
          }
          return HttpResponse.json(entry);
        }),
      );

      render(<DeepLinkImportDialog />, { wrapper: Wrapper });
      await hit;
      act(() => {
        deliverDeepLink(providerLink("Newer Provider").request);
      });
      // 给并发读取（如果有）留出完成的时间
      await new Promise((resolve) => setTimeout(resolve, 30));
      release();

      await waitFor(() => {
        expect(screen.getByText("Newer Provider")).toBeInTheDocument();
      });
      await waitFor(() => expect(peekPendingDeepLink()).toBeNull());
      expect(screen.queryByText("Older Provider")).not.toBeInTheDocument();
    });

    it("shows the newer link when it arrives while the older one is merging its config", async () => {
      // 读取已返回 A，A 带 config 正在合并（异步），此时 B 到达：最终必须停在 B
      setPendingDeepLink({
        ...coldStartLink,
        request: {
          ...coldStartLink.request,
          name: "Merging Provider",
          config: btoa('{"env":{}}'),
        },
      });

      let release!: () => void;
      const gate = new Promise<void>((resolve) => (release = resolve));
      let markHit!: () => void;
      const hit = new Promise<void>((resolve) => (markHit = resolve));
      server.use(
        http.post(
          "http://tauri.local/merge_deeplink_config",
          async ({ request }) => {
            const body = (await request.json()) as {
              request: Record<string, unknown>;
            };
            markHit();
            await gate;
            return HttpResponse.json(body.request);
          },
        ),
      );

      render(<DeepLinkImportDialog />, { wrapper: Wrapper });
      await hit;
      act(() => {
        deliverDeepLink({
          ...coldStartLink.request,
          name: "Arrived During Merge",
        });
      });
      await new Promise((resolve) => setTimeout(resolve, 30));
      release();

      await waitFor(() => {
        expect(screen.getByText("Arrived During Merge")).toBeInTheDocument();
      });
      await waitFor(() => expect(peekPendingDeepLink()).toBeNull());
      expect(screen.queryByText("Merging Provider")).not.toBeInTheDocument();
    });

    it("keeps a newer link's confirmation open when an older import finishes", async () => {
      // A 导入中（响应挂起）时 B 到达并显示、后端已清除 B；A 完成后不能把 B 的确认框关掉
      const user = userEvent.setup();
      const providerLink = (name: string) => ({
        ...coldStartLink.request,
        name,
      });
      let release!: () => void;
      const gate = new Promise<void>((resolve) => (release = resolve));
      let markHit!: () => void;
      const hit = new Promise<void>((resolve) => (markHit = resolve));
      const imported: string[] = [];
      server.use(
        http.post(
          "http://tauri.local/import_from_deeplink_unified",
          async ({ request }) => {
            const body = (await request.json()) as {
              request: { name: string };
            };
            imported.push(body.request.name);
            if (imported.length === 1) {
              markHit();
              await gate;
            }
            return HttpResponse.json({ type: "provider", id: "p1" });
          },
        ),
      );

      render(<DeepLinkImportDialog />, { wrapper: Wrapper });
      act(() => deliverDeepLink(providerLink("First Provider")));
      await user.click(
        await screen.findByRole("button", { name: "deeplink.import" }),
      );
      await hit;

      act(() => deliverDeepLink(providerLink("Second Provider")));
      await screen.findByText("Second Provider");
      await waitFor(() => expect(peekPendingDeepLink()).toBeNull());

      release();
      await waitFor(() =>
        expect(
          screen.getByRole("button", { name: "deeplink.import" }),
        ).toBeEnabled(),
      );
      const dialog = screen.getByTestId("deeplink-dialog");
      expect(dialog).toHaveAttribute("data-open", "true");
      expect(screen.getByText("Second Provider")).toBeInTheDocument();

      // B 自己导入完成后照常关闭
      await user.click(screen.getByRole("button", { name: "deeplink.import" }));
      await waitFor(() => expect(dialog).toHaveAttribute("data-open", "false"));
      expect(imported).toEqual(["First Provider", "Second Provider"]);
    });
  });
});

it("previews agy model and masks its key before confirmation", async () => {
  server.use(
    http.post(
      "http://tauri.local/merge_deeplink_config",
      async ({ request }) => {
        const body = (await request.json()) as {
          request: Record<string, unknown>;
        };
        return HttpResponse.json(body.request);
      },
    ),
  );
  render(<DeepLinkImportDialog />, { wrapper: Wrapper });
  act(() => {
    deliverDeepLink({
      version: "v1",
      resource: "provider",
      app: "agy",
      name: "Agy fixture",
      endpoint: "https://relay.example",
      apiKey: "secret-agy-key",
      config: btoa(
        JSON.stringify({
          env: { GEMINI_API_KEY: "secret-agy-key" },
          config: { model: { name: "agy-preview-model" } },
        }),
      ),
    });
  });
  await waitFor(() =>
    expect(screen.getByText("agy-preview-model")).toBeInTheDocument(),
  );
  expect(screen.queryByText("secret-agy-key")).not.toBeInTheDocument();
  expect(screen.getAllByText("secr************").length).toBeGreaterThan(0);
});
