import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import type { Provider } from "@/types";
import type {
  AppModeView,
  ProxyStatus,
  PublicRouteStatus,
} from "@/types/proxy";
import {
  CURSOR_AUTO_MODEL_IDS,
  cursorProviderPresets,
  type CursorProviderPreset,
} from "@/config/cursorProviderPresets";
import { proxyApi } from "@/lib/api/proxy";
import { useModeActions } from "@/hooks/useModeActions";
import { SwitchModePanel } from "@/components/providers/mode/SwitchModePanel";
import { CursorSetupDialog } from "@/components/providers/mode/CursorSetupDialog";

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    info: vi.fn(),
    warning: vi.fn(),
    error: vi.fn(),
  },
}));

// 只打桩 IPC：断言「调用了什么、参数是什么」，同时保留真实实现给没被覆盖的命令。
vi.mock("@/lib/api/proxy", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api/proxy")>();
  return {
    proxyApi: {
      ...actual.proxyApi,
      getAppMode: vi.fn(),
      getProxyStatus: vi.fn(),
      getPublicRouteStatus: vi.fn(),
      enablePublicRoute: vi.fn(),
      cursorIsRunning: vi.fn(),
      cursorByokStatus: vi.fn(),
      cursorWriteByok: vi.fn(),
      testPublicRouteConnectivity: vi.fn(),
    },
  };
});

// 面板/对话框都不许「代开」：把进入模式的入口留一个可断言的探针
const enterSpy = vi.fn();
vi.mock("@/hooks/useModeActions", () => ({
  useModeActions: vi.fn(),
}));

vi.mock("@/components/providers/ProviderCard", () => ({
  ProviderCard: ({ provider, presentation }: any) => (
    <div data-testid={`card-${provider.id}`}>
      {presentation.buttons.map((button: any) => (
        <button
          key={button.key}
          data-testid={`${button.key}-${provider.id}`}
          disabled={Boolean(button.disabledReason)}
          onClick={button.onClick}
        >
          {button.label}
        </button>
      ))}
    </div>
  ),
}));

vi.mock("@/hooks/useStreamCheck", () => ({
  useStreamCheck: () => ({ checkProvider: vi.fn(), isChecking: () => false }),
}));

const MODE: AppModeView = {
  mode: "route",
  attached: true,
  routeProviderId: "kimi",
  directProviderId: "kimi",
};

const PROXY_STATUS = {
  running: true,
  address: "127.0.0.1",
  port: 15721,
} as unknown as ProxyStatus;

const READY_TUNNEL: PublicRouteStatus = {
  enabled: true,
  apiKey: "ccsk-test-key",
  tunnelRunning: true,
  publicUrl: "https://example.trycloudflare.com",
  localUrl: "http://127.0.0.1:15721",
  tunnelError: null,
  currentProviderName: "Kimi",
  tunnelMode: "quick",
  namedTunnel: null,
  namedHostname: null,
};

const PROVIDER: Provider = {
  id: "kimi",
  name: "Kimi",
  category: "cn_official",
  settingsConfig: {
    baseUrl: "https://api.moonshot.cn/v1",
    apiKey: "sk-x",
    model: "kimi-k2.7-code",
    modelCatalog: {
      models: [
        { model: "kimi-k2.7-code", displayName: "Kimi", contextWindow: "" },
      ],
    },
  },
};

/** 路由视图里的另一家非官方供应商：卡片上带「路由到此」入口。 */
const OTHER: Provider = {
  id: "other",
  name: "Other",
  category: "cn_official",
  settingsConfig: { baseUrl: "https://api.other.example/v1", apiKey: "sk-y" },
};

/** Cursor 官方账号卡：不需要接入引导，照旧一步切过去。 */
const OFFICIAL: Provider = {
  id: "official",
  name: "Cursor 官方",
  category: "official",
  settingsConfig: {},
};

/**
 * 用内置预设造 Provider fixture：预设的 `modelCatalog` 在**顶层**，而组件读的是
 * `settingsConfig.modelCatalog.models`。这里挪进去的方式与 ProviderForm 选择 Cursor 预设时一致
 * （见 ProviderForm.tsx 的 cursor 分支：`config.modelCatalog = { models: preset.modelCatalog }`）。
 */
function providerFromPreset(
  preset: CursorProviderPreset,
  id: string,
): Provider {
  return {
    id,
    name: preset.name,
    category: preset.category,
    settingsConfig: {
      ...preset.settingsConfig,
      ...(preset.modelCatalog?.length
        ? { modelCatalog: { models: preset.modelCatalog } }
        : {}),
    },
  };
}

/** 按名字取内置 Cursor 预设；找不到直接抛，免得测试悄悄退化成空目录还自称「用了真实列表」。 */
function requirePreset(name: string): CursorProviderPreset {
  const preset = cursorProviderPresets.find((item) => item.name === name);
  if (!preset) throw new Error(`内置预设不存在：${name}`);
  return preset;
}

/** fixture 里模型目录的客户端模型名（与组件 cursorClientModelIds 的取值口径一致）。 */
function catalogModelIds(provider: Provider): string[] {
  const models = provider.settingsConfig.modelCatalog?.models as
    | Array<{ model: string }>
    | undefined;
  return (models ?? []).map((row) => row.model);
}

const writeText = vi.fn();

function renderDialog(
  overrides: Partial<{
    provider: Provider;
    onClose: () => void;
    onSwitch: (provider: Provider) => unknown;
    onEnableLocalRoute: () => void;
    onOpenPublicRouteSettings: () => void;
    onOpenRoutingSettings: () => void;
  }> = {},
) {
  const props = {
    provider: PROVIDER,
    onClose: vi.fn(),
    onSwitch: vi.fn(),
    onEnableLocalRoute: vi.fn(),
    onOpenPublicRouteSettings: vi.fn(),
    onOpenRoutingSettings: vi.fn(),
    ...overrides,
  };
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const view = render(
    createElement(
      QueryClientProvider,
      { client: queryClient },
      createElement(CursorSetupDialog, props),
    ),
  );
  return { ...view, props };
}

beforeEach(() => {
  vi.mocked(useModeActions).mockReturnValue({
    enter: enterSpy,
    exitToDirect: vi.fn(),
    pending: false,
    restartHint: "",
  });
  vi.mocked(proxyApi.getAppMode).mockResolvedValue(MODE);
  vi.mocked(proxyApi.getProxyStatus).mockResolvedValue(PROXY_STATUS);
  vi.mocked(proxyApi.getPublicRouteStatus).mockResolvedValue(READY_TUNNEL);
  vi.mocked(proxyApi.cursorIsRunning).mockResolvedValue(false);
  vi.mocked(proxyApi.cursorByokStatus).mockResolvedValue({
    baseUrl: null,
    useOpenAIKey: false,
    modelCount: 0,
    modelIds: [],
  });
  vi.mocked(proxyApi.cursorWriteByok).mockResolvedValue({
    ok: true,
    backupPath: "/tmp/backup.json",
    writtenBaseUrl: "https://example.trycloudflare.com/cursor/v1",
  });
  vi.mocked(proxyApi.testPublicRouteConnectivity).mockResolvedValue({
    ok: true,
    status: 200,
    detail: "",
  });
  writeText.mockReset();
  writeText.mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", {
    value: { writeText },
    configurable: true,
  });
});

afterEach(() => {
  vi.useRealTimers();
});

describe("CursorSetupDialog — 四张卡", () => {
  it("卡 A：本地路由没开时只提示开路由，且不执行切换", async () => {
    vi.mocked(proxyApi.getAppMode).mockResolvedValue({
      ...MODE,
      mode: "direct",
    });
    const { props } = renderDialog();

    expect(
      await screen.findByTestId("cursor-setup-localRoute"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("cursor-setup-publicRoute"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("cursor-setup-exitCursor"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("cursor-setup-pasteKey"),
    ).not.toBeInTheDocument();
    expect(props.onSwitch).not.toHaveBeenCalled();
  });

  it("卡 B：公网路由没就绪时提示去设置，并把隧道失败原因显示出来", async () => {
    vi.mocked(proxyApi.getPublicRouteStatus).mockResolvedValue({
      ...READY_TUNNEL,
      enabled: false,
      tunnelRunning: false,
      publicUrl: null,
      apiKey: null,
      tunnelError: "启动超时，未获取到公网地址",
    });
    const { props } = renderDialog();

    const card = await screen.findByTestId("cursor-setup-publicRoute");
    expect(card).toBeInTheDocument();
    expect(card).toHaveTextContent("启动超时，未获取到公网地址");
    expect(
      screen.queryByTestId("cursor-setup-localRoute"),
    ).not.toBeInTheDocument();
    expect(props.onSwitch).not.toHaveBeenCalled();
  });

  it("卡 C：Cursor 正在运行时只提示退出，**此时还不切换**", async () => {
    // ⚠️ 需求已改（2026-10-13 用户实测）：原先「等用户退 Cursor 时就先把供应商切过去」，
    // 用户看到的顺序是「公网路由还没开/还没写 BYOK，供应商已经启用了」。要求的顺序是
    // 前提 → 退出 Cursor → 写 BYOK → **最后**才启用。这不是放宽断言，是纠正顺序。
    vi.mocked(proxyApi.cursorIsRunning).mockResolvedValue(true);
    const { props } = renderDialog();

    expect(
      await screen.findByTestId("cursor-setup-exitCursor"),
    ).toBeInTheDocument();
    expect(props.onSwitch).not.toHaveBeenCalled();
    expect(proxyApi.cursorWriteByok).not.toHaveBeenCalled();
    expect(
      screen.queryByTestId("cursor-setup-pasteKey"),
    ).not.toBeInTheDocument();
  });

  it("卡 D：写入 BYOK 后只差密钥，按供应商模型目录写 modelIds", async () => {
    renderDialog();

    expect(
      await screen.findByTestId("cursor-setup-pasteKey"),
    ).toBeInTheDocument();
    expect(proxyApi.cursorWriteByok).toHaveBeenCalledWith({
      baseUrl: "https://example.trycloudflare.com/cursor/v1",
      modelIds: ["kimi-k2.7-code"],
    });
  });

  it("已经写好的 BYOK 不再重复写，直接停在卡 D", async () => {
    // fixture 更新（预期内）：判定「已写好」现在还要比对具体 modelIds 集合，故这里的 mock
    // 必须给出与本供应商目录一致的列表，否则会被判成「没写好」而重写。
    vi.mocked(proxyApi.cursorByokStatus).mockResolvedValue({
      baseUrl: "https://example.trycloudflare.com/cursor/v1",
      useOpenAIKey: true,
      modelCount: 1,
      modelIds: ["kimi-k2.7-code"],
    });
    renderDialog();

    expect(
      await screen.findByTestId("cursor-setup-pasteKey"),
    ).toBeInTheDocument();
    expect(proxyApi.cursorWriteByok).not.toHaveBeenCalled();
  });

  it("同一公网隧道下换供应商：模型目录不同就必须重写 modelIds", async () => {
    // 回归（P2）：仍走同一条隧道时 baseUrl 不变，旧实现只看 baseUrl + modelCount>0 就判「已写好」，
    // 于是 B 家的模型列表永远写不进 Cursor（Cursor 继续显示、甚至继续发 A 家的模型 ID）。
    // 核心不变式：已写入列表非空 ≠ 目标已写入。
    // 这里用**两个内置预设的真实目录**：上次写进 Cursor 的是 Kimi，这次要换成 Kimi For Coding。
    const previousModels = catalogModelIds(
      providerFromPreset(requirePreset("Kimi"), "kimi"),
    );
    // 先确认取到的确实是预设里的真实列表（预设改动会让本测试显形，而不是悄悄变空目录走回退）
    expect(previousModels).toEqual(["kimi-k2.7-code", "kimi-k3"]);

    vi.mocked(proxyApi.cursorByokStatus).mockResolvedValue({
      baseUrl: "https://example.trycloudflare.com/cursor/v1",
      useOpenAIKey: true,
      modelCount: previousModels.length,
      modelIds: previousModels, // 上次写入的是 Kimi 的真实模型列表
    });
    renderDialog({
      provider: providerFromPreset(
        requirePreset("Kimi For Coding"),
        "kimi-coding",
      ),
    });

    await screen.findByTestId("cursor-setup-pasteKey");
    expect(proxyApi.cursorWriteByok).toHaveBeenCalledWith({
      baseUrl: "https://example.trycloudflare.com/cursor/v1",
      modelIds: ["kimi-for-coding"], // Kimi For Coding 预设的真实列表
    });
  });

  it("已写入列表顺序不同但集合相同：不重复写（比较须与顺序无关）", async () => {
    // Cursor 侧数组顺序不保证，集合相同就应视为「已写好」。这里故意把顺序反过来。
    const twoModels: Provider = {
      id: "two",
      name: "Two",
      settingsConfig: {
        modelCatalog: {
          models: [
            { model: "m-alpha", displayName: "A", contextWindow: "" },
            { model: "m-beta", displayName: "B", contextWindow: "" },
          ],
        },
      },
    };
    vi.mocked(proxyApi.cursorByokStatus).mockResolvedValue({
      baseUrl: "https://example.trycloudflare.com/cursor/v1",
      useOpenAIKey: true,
      modelCount: 2,
      modelIds: ["m-beta", "m-alpha"],
    });
    renderDialog({ provider: twoModels });

    await screen.findByTestId("cursor-setup-pasteKey");
    expect(proxyApi.cursorWriteByok).not.toHaveBeenCalled();
  });
});

describe("CursorSetupDialog — 只跳转、不代开", () => {
  it("卡 A 的主按钮就地开启 Cursor 接管：不换供应商、不关框", async () => {
    // ⚠️ 需求已改（2026-10-13 用户实测「公网路由没开，供应商已经启用了」）：原先卡 A 是
    // 「只跳转到路由视图」。但现有 UI 里进入路由模式**必须选一家做路由目标**，选那家就等于
    // 提前启用它，与用户要求的「最后才启用供应商」直接冲突（带「应用接管」开关的 ProxyPanel
    // 是死代码，界面上到不了）。改成就地开启 Cursor 接管、路由目标沿用上次——那家不因此被启用；
    // 真正的启用留到卡 D 写完之后。这不是放宽断言，是纠正顺序。
    vi.mocked(proxyApi.getAppMode).mockResolvedValue({
      ...MODE,
      mode: "direct",
    });
    const { props } = renderDialog();
    fireEvent.click(
      await screen.findByRole("button", { name: "cursorSetup.cardA.action" }),
    );

    expect(props.onEnableLocalRoute).toHaveBeenCalledTimes(1);
    expect(props.onClose).not.toHaveBeenCalled();
    expect(enterSpy).not.toHaveBeenCalled();
    expect(props.onSwitch).not.toHaveBeenCalled();
    expect(proxyApi.enablePublicRoute).not.toHaveBeenCalled();
  });

  it("卡 B 的主按钮只打开公网路由设置并关框", async () => {
    vi.mocked(proxyApi.getPublicRouteStatus).mockResolvedValue({
      ...READY_TUNNEL,
      enabled: false,
      tunnelRunning: false,
      publicUrl: null,
      apiKey: null,
    });
    const { props } = renderDialog();
    fireEvent.click(
      await screen.findByRole("button", { name: "cursorSetup.cardB.action" }),
    );

    // 必须跳到**公网路由**那一栏（settings 的 `publicRoute` section），不是本地路由那一栏：
    // 2026-10-13 用户实测「点『打开公网路由设置』跳到的是本地路由设置页面」——
    // 上一轮只断言了「回调被调用」，没验**跳到哪**，所以跳错页面测试照样绿。
    expect(props.onOpenPublicRouteSettings).toHaveBeenCalledTimes(1);
    expect(props.onOpenRoutingSettings).not.toHaveBeenCalled();
    expect(props.onClose).toHaveBeenCalledTimes(1);
    expect(enterSpy).not.toHaveBeenCalled();
    expect(proxyApi.enablePublicRoute).not.toHaveBeenCalled();
  });
});

describe("CursorSetupDialog — 轮询与复制", () => {
  it("卡 C 每 3 秒轮询一次，Cursor 退出后自动继续到写 BYOK", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    let running = true;
    vi.mocked(proxyApi.cursorIsRunning).mockImplementation(async () => running);
    renderDialog();

    expect(
      await screen.findByTestId("cursor-setup-exitCursor"),
    ).toBeInTheDocument();
    expect(proxyApi.cursorIsRunning).toHaveBeenCalledTimes(1);

    running = false;
    await act(async () => {
      vi.advanceTimersByTime(3000);
    });

    expect(
      await screen.findByTestId("cursor-setup-pasteKey"),
    ).toBeInTheDocument();
    expect(proxyApi.cursorIsRunning).toHaveBeenCalledTimes(2);
    expect(proxyApi.cursorWriteByok).toHaveBeenCalledTimes(1);
  });

  it("顺序：公网路由就绪后先等 Cursor 退出、写 BYOK，最后才启用供应商（切换）", async () => {
    // 2026-10-13 用户实测：原本「先切换供应商、再开公网路由/写 BYOK」，用户看到的是
    // 「公网路由还没开，供应商已经启用了」。要求的顺序是：前提 → 退出 Cursor → 写 BYOK → 最后启用。
    const order: string[] = [];
    vi.mocked(proxyApi.cursorWriteByok).mockImplementation(async () => {
      order.push("writeByok");
      return { ok: true, backupPath: null, writtenBaseUrl: null };
    });
    renderDialog({
      onSwitch: () => {
        order.push("switch");
      },
    });

    await screen.findByTestId("cursor-setup-pasteKey");
    expect(order).toEqual(["writeByok", "switch"]);
  });

  it("卡 D 的复制按钮把密钥写进剪贴板", async () => {
    renderDialog();
    fireEvent.click(
      await screen.findByRole("button", { name: "cursorSetup.cardD.copy" }),
    );

    expect(writeText).toHaveBeenCalledWith("ccsk-test-key");
    expect(
      await screen.findByTestId("cursor-setup-pasteKey"),
    ).toHaveTextContent("Cursor → Settings → Models → OpenAI API Key");
  });

  it("剪贴板失败时不谎报成功", async () => {
    writeText.mockRejectedValue(new Error("denied"));
    renderDialog();
    fireEvent.click(
      await screen.findByRole("button", { name: "cursorSetup.cardD.copy" }),
    );

    expect(
      await screen.findByText("cursorSetup.cardD.copyFailed"),
    ).toBeInTheDocument();
    expect(vi.mocked(toast.success)).not.toHaveBeenCalledWith(
      "cursorSetup.cardD.copied",
    );
    expect(vi.mocked(toast.error)).toHaveBeenCalled();
  });
});

describe("CursorSetupDialog — 步骤条、回退与失败路径", () => {
  it("步骤条按缺口标 ✓ / ● / ○", async () => {
    vi.mocked(proxyApi.getAppMode).mockResolvedValue({
      ...MODE,
      mode: "direct",
    });
    const view = renderDialog();

    await screen.findByTestId("cursor-setup-localRoute");
    expect(screen.getByTestId("cursor-setup-step-localRoute")).toHaveAttribute(
      "data-state",
      "current",
    );
    expect(screen.getByTestId("cursor-setup-step-publicRoute")).toHaveAttribute(
      "data-state",
      "todo",
    );
    view.unmount();

    vi.mocked(proxyApi.getAppMode).mockResolvedValue(MODE);
    renderDialog();
    await screen.findByTestId("cursor-setup-pasteKey");
    // 只差密钥时，前三步都算已满足
    for (const step of ["localRoute", "publicRoute", "exitCursor"]) {
      expect(screen.getByTestId(`cursor-setup-step-${step}`)).toHaveAttribute(
        "data-state",
        "done",
      );
    }
    expect(screen.getByTestId("cursor-setup-step-pasteKey")).toHaveAttribute(
      "data-state",
      "current",
    );
  });

  it("供应商没有模型目录时回退到预填的客户端模型名", async () => {
    renderDialog({
      provider: { id: "kimi", name: "Kimi", settingsConfig: {} },
    });

    await screen.findByTestId("cursor-setup-pasteKey");
    expect(proxyApi.cursorWriteByok).toHaveBeenCalledWith({
      baseUrl: "https://example.trycloudflare.com/cursor/v1",
      modelIds: [...CURSOR_AUTO_MODEL_IDS],
    });
  });

  it("写 BYOK 失败时停在失败卡并显示原因，重试成功后进卡 D", async () => {
    vi.mocked(proxyApi.cursorWriteByok)
      .mockRejectedValueOnce(new Error("写入被拒"))
      .mockResolvedValueOnce({
        ok: true,
        backupPath: null,
        writtenBaseUrl: null,
      });
    renderDialog();

    const failed = await screen.findByTestId("cursor-setup-writeFailed");
    expect(failed).toHaveTextContent("写入被拒");

    fireEvent.click(
      screen.getByRole("button", { name: "cursorSetup.writeFailed.retry" }),
    );

    expect(
      await screen.findByTestId("cursor-setup-pasteKey"),
    ).toBeInTheDocument();
    expect(proxyApi.cursorWriteByok).toHaveBeenCalledTimes(2);
  });

  it("卡 C 的「稍后再补」只关框，不做任何静默补写", async () => {
    vi.mocked(proxyApi.cursorIsRunning).mockResolvedValue(true);
    const { props } = renderDialog();
    fireEvent.click(
      await screen.findByRole("button", { name: "cursorSetup.cardC.action" }),
    );

    expect(props.onClose).toHaveBeenCalledTimes(1);
    expect(proxyApi.cursorWriteByok).not.toHaveBeenCalled();
  });

  it("没有公网密钥时不谎报复制成功", async () => {
    vi.mocked(proxyApi.getPublicRouteStatus).mockResolvedValue({
      ...READY_TUNNEL,
      apiKey: null,
    });
    renderDialog();
    fireEvent.click(
      await screen.findByRole("button", { name: "cursorSetup.cardD.copy" }),
    );

    expect(writeText).not.toHaveBeenCalled();
    expect(
      await screen.findByText("cursorSetup.cardD.copyFailed"),
    ).toBeInTheDocument();
    expect(vi.mocked(toast.success)).not.toHaveBeenCalled();
  });
});

// 触发点：卡上「切换」按钮改走引导框，只有 cursor 非官方供应商走这条路
function renderPanel(
  app: "cursor" | "claude",
  providers: Record<string, Provider>,
  onSwitch = vi.fn(),
) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return {
    onSwitch,
    ...render(
      createElement(
        QueryClientProvider,
        { client: queryClient },
        createElement(SwitchModePanel, {
          app,
          providers,
          currentProviderId: Object.keys(providers)[0] ?? "",
          isLoading: false,
          onSwitch,
          onOpenRoutingSettings: vi.fn(),
          onOpenPublicRouteSettings: vi.fn(),
          onEdit: vi.fn(),
          onDelete: vi.fn(),
          onDuplicate: vi.fn(),
          onOpenWebsite: vi.fn(),
          // 上游 v7 把生效状态徽标改成 portal 到页头槽位；测试里挂到 body，供 findByLabelText 等查询
          statusSlot: document.body as unknown as HTMLDivElement,
        }),
      ),
    ),
  };
}

describe("SwitchModePanel — Cursor 接入引导触发点", () => {
  it("cursor 非官方供应商的「切换」先开引导框，不直接切换", async () => {
    vi.mocked(proxyApi.getAppMode).mockResolvedValue({
      ...MODE,
      mode: "direct",
    });
    const { onSwitch } = renderPanel("cursor", { kimi: PROVIDER });

    fireEvent.click(await screen.findByTestId("switch-kimi"));

    expect(
      await screen.findByTestId("cursor-setup-localRoute"),
    ).toBeInTheDocument();
    expect(onSwitch).not.toHaveBeenCalled();
    expect(enterSpy).not.toHaveBeenCalled();
    expect(proxyApi.enablePublicRoute).not.toHaveBeenCalled();
  });

  it("其它应用仍然直接切换，不弹引导框", async () => {
    vi.mocked(proxyApi.getAppMode).mockResolvedValue({
      ...MODE,
      mode: "direct",
    });
    const { onSwitch } = renderPanel("claude", { kimi: PROVIDER });

    fireEvent.click(await screen.findByTestId("switch-kimi"));

    expect(onSwitch).toHaveBeenCalledTimes(1);
    expect(
      screen.queryByTestId("cursor-setup-localRoute"),
    ).not.toBeInTheDocument();
  });

  // 路由视图的「路由到此」是并列入口：Cursor 用户因历史原因默认落在这格，
  // 它必须和直连视图的「切换」一样先过接入引导。
  it("路由视图点「路由到此」：cursor 非官方先开引导框，不直接切换", async () => {
    vi.mocked(proxyApi.getAppMode).mockResolvedValue({
      ...MODE,
      mode: "route",
    });
    // 公网路由没打开（正是用户报的那个 bug 场景）：对话框只提示去开路由，不切换
    vi.mocked(proxyApi.getPublicRouteStatus).mockResolvedValue({
      ...READY_TUNNEL,
      enabled: false,
      tunnelRunning: false,
      publicUrl: null,
      apiKey: null,
    });
    const { onSwitch } = renderPanel("cursor", {
      kimi: PROVIDER,
      other: OTHER,
    });

    fireEvent.click(await screen.findByTestId("routeHere-other"));

    expect(await screen.findByTestId("cursor-setup-steps")).toBeInTheDocument();
    expect(onSwitch).not.toHaveBeenCalled();
    expect(
      screen.queryByTestId("cursor-setup-localRoute"),
    ).not.toBeInTheDocument();
  });

  it("路由视图点「路由到此」：其它应用仍然直接切换", async () => {
    vi.mocked(proxyApi.getAppMode).mockResolvedValue({
      ...MODE,
      mode: "route",
    });
    const { onSwitch } = renderPanel("claude", {
      kimi: PROVIDER,
      other: OTHER,
    });

    fireEvent.click(await screen.findByTestId("routeHere-other"));

    expect(onSwitch).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("cursor-setup-steps")).not.toBeInTheDocument();
  });

  it("cursor 官方供应商仍然直接切换，不被一刀切拦下", async () => {
    vi.mocked(proxyApi.getAppMode).mockResolvedValue({
      ...MODE,
      mode: "direct",
    });
    const { onSwitch } = renderPanel("cursor", {
      kimi: PROVIDER,
      official: OFFICIAL,
    });

    fireEvent.click(await screen.findByTestId("switch-official"));

    expect(onSwitch).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("cursor-setup-steps")).not.toBeInTheDocument();
  });

  it("托盘请求指向 cursor 非官方时也走接入引导，而不是「需要路由」框", async () => {
    vi.mocked(proxyApi.getAppMode).mockResolvedValue({
      ...MODE,
      mode: "direct",
    });
    const onHandled = vi.fn();
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      createElement(
        QueryClientProvider,
        { client: queryClient },
        createElement(SwitchModePanel, {
          app: "cursor",
          providers: { kimi: PROVIDER, other: OTHER },
          currentProviderId: "kimi",
          isLoading: false,
          onSwitch: vi.fn(),
          onOpenRoutingSettings: vi.fn(),
          onOpenPublicRouteSettings: vi.fn(),
          onEdit: vi.fn(),
          onDelete: vi.fn(),
          onDuplicate: vi.fn(),
          onOpenWebsite: vi.fn(),
          needsRouteRequest: { providerId: "other", nonce: 1 },
          onNeedsRouteHandled: onHandled,
        }),
      ),
    );

    expect(
      await screen.findByTestId("cursor-setup-localRoute"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("mode.dialog.needsRouteTitle"),
    ).not.toBeInTheDocument();
    expect(onHandled).toHaveBeenCalledTimes(1);
  });
});

// 「进入路由模式」框（ModeDialog kind:"enter"）与卡片上的「回到直连并使用」是两条并列的
// 启用入口：上一轮只给卡片按钮加了闸，这两条绕过了接入引导 —— 本组用例把它们钉死。
describe("SwitchModePanel — 进入路由 / 回到直连并使用 也要过接入引导", () => {
  it("「进入路由模式」框里选 cursor 非官方：照常进入路由，并在进去之后接着引导", async () => {
    // ⚠️ 需求已改（2026-10-13 用户实测报死循环）：原先要求这条**也过闸**，但卡 A 的按钮
    // 就是让用户「去开启路由」，而用户真去开启时走的正是这条动作 → 被闸拦回、又弹回同一张
    // 卡 A，点多少次都出不去。正解：这条动作**本身就是用户在显式做前提 1**，不能拦；
    // 进去之后由引导框接着报下一个缺口（公网路由）。这不是放宽断言，是纠正上一轮定错的闸门范围。
    vi.mocked(proxyApi.getAppMode).mockResolvedValue({
      ...MODE,
      mode: "direct",
      routeProviderId: null,
      directProviderId: "other",
    });
    const { onSwitch } = renderPanel("cursor", {
      kimi: PROVIDER,
      other: OTHER,
    });

    // 切到「路由」页签 → 出现激活条 → 打开「进入路由模式」框（预选直连那家 other）
    fireEvent.click(
      await screen.findByRole("button", { name: /mode\.names\.route/ }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "mode.activate.route" }),
    );
    fireEvent.click(await screen.findByText("mode.dialog.confirmRoute"));

    // 真的进入了路由（不再被拦）
    await waitFor(() =>
      expect(enterSpy).toHaveBeenCalledWith("route", "other", "Other"),
    );
    // 而且引导框继续在（接着报下一个缺口），用户不会卡在「点了没反应/反复弹同一张卡」
    expect(await screen.findByTestId("cursor-setup-steps")).toBeInTheDocument();
    expect(onSwitch).not.toHaveBeenCalled();
  });

  it("「回到直连并使用」点 cursor 非官方：开接入引导框，不调 exitToDirect", async () => {
    const exitSpy = vi.fn();
    vi.mocked(useModeActions).mockReturnValue({
      enter: enterSpy,
      exitToDirect: exitSpy,
      pending: false,
      restartHint: "",
    });
    vi.mocked(proxyApi.getAppMode).mockResolvedValue({
      ...MODE,
      mode: "route",
      routeProviderId: "kimi",
      directProviderId: "kimi",
    });
    const { onSwitch } = renderPanel("cursor", {
      kimi: PROVIDER,
      other: OTHER,
    });

    // 等模式读到再切到「直连」页签：卡片上的「回到直连并使用」才会渲染
    // 页头始终显示当前生效连接（上游 v7 改版后不再拼「→ 」前缀）
    await screen.findByLabelText("mode.status.routeLead Kimi");
    fireEvent.click(
      screen.getByRole("button", { name: /mode\.names\.direct/ }),
    );
    fireEvent.click(await screen.findByTestId("exitAndUse-other"));

    expect(await screen.findByTestId("cursor-setup-steps")).toBeInTheDocument();
    expect(exitSpy).not.toHaveBeenCalled();
    expect(onSwitch).not.toHaveBeenCalled();
  });

  it("「进入路由模式」框里选其它应用的普通供应商：照常进入路由", async () => {
    vi.mocked(proxyApi.getAppMode).mockResolvedValue({
      ...MODE,
      mode: "direct",
      routeProviderId: null,
      directProviderId: "other",
    });
    renderPanel("claude", { kimi: PROVIDER, other: OTHER });

    fireEvent.click(
      await screen.findByRole("button", { name: /mode\.names\.route/ }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "mode.activate.route" }),
    );
    fireEvent.click(await screen.findByText("mode.dialog.confirmRoute"));

    await waitFor(() =>
      expect(enterSpy).toHaveBeenCalledWith("route", "other", "Other"),
    );
    expect(screen.queryByTestId("cursor-setup-steps")).not.toBeInTheDocument();
  });

  it("启动 attach 失败的「重试」点 cursor 非官方：开接入引导框，不重试重连", async () => {
    vi.mocked(proxyApi.getAppMode).mockResolvedValue({
      ...MODE,
      mode: "route",
      routeProviderId: "other",
      directProviderId: "kimi",
    });
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    render(
      createElement(
        QueryClientProvider,
        { client: queryClient },
        createElement(SwitchModePanel, {
          app: "cursor",
          providers: { kimi: PROVIDER, other: OTHER },
          currentProviderId: "kimi",
          isLoading: false,
          onSwitch: vi.fn(),
          onOpenRoutingSettings: vi.fn(),
          onOpenPublicRouteSettings: vi.fn(),
          onEdit: vi.fn(),
          onDelete: vi.fn(),
          onDuplicate: vi.fn(),
          onOpenWebsite: vi.fn(),
          startupFailure: {
            appType: "cursor",
            stack: false,
            error: "attach 失败",
          },
          statusSlot: document.body as unknown as HTMLDivElement,
        }),
      ),
    );

    // 等模式读到（routeId 有了）再点重试：重试要按当前路由目标那家过闸
    await screen.findByLabelText("mode.status.routeLead Other");
    fireEvent.click(
      await screen.findByRole("button", { name: "common.retry" }),
    );

    expect(await screen.findByTestId("cursor-setup-steps")).toBeInTheDocument();
    expect(enterSpy).not.toHaveBeenCalled();
  });
});

describe("CursorSetupDialog — 卡 D 的分层连通性检查", () => {
  it("自动按 local → public 分层跑：local 通过才继续测 public", async () => {
    const order: string[] = [];
    vi.mocked(proxyApi.testPublicRouteConnectivity).mockImplementation(
      async (scope) => {
        order.push(scope);
        return { ok: true, status: 200, detail: "" };
      },
    );
    renderDialog();

    await screen.findByTestId("cursor-setup-connectivity-ok");
    expect(order).toEqual(["local", "public"]);
  });

  it("local 不通就不再测 public，并提示坏在本地路由这一层", async () => {
    const order: string[] = [];
    vi.mocked(proxyApi.testPublicRouteConnectivity).mockImplementation(
      async (scope) => {
        order.push(scope);
        return { ok: false, status: null, detail: "本地代理未启动" };
      },
    );
    renderDialog();

    const card = await screen.findByTestId(
      "cursor-setup-connectivity-localFailed",
    );
    expect(card).toHaveTextContent("cursorSetup.connectivity.localFailed");
    expect(card).toHaveTextContent("本地代理未启动");
    expect(order).toEqual(["local"]);
    expect(proxyApi.testPublicRouteConnectivity).toHaveBeenCalledTimes(1);
    expect(
      screen.queryByTestId("cursor-setup-connectivity-publicFailed"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("cursor-setup-connectivity-ok"),
    ).not.toBeInTheDocument();
  });

  it("public 不通时提示坏在公网隧道这一层，与本地路由的提示不同", async () => {
    vi.mocked(proxyApi.testPublicRouteConnectivity).mockImplementation(
      async (scope) =>
        scope === "local"
          ? { ok: true, status: 200, detail: "" }
          : { ok: false, status: 502, detail: "隧道不回包" },
    );
    renderDialog();

    const card = await screen.findByTestId(
      "cursor-setup-connectivity-publicFailed",
    );
    expect(card).toHaveTextContent("cursorSetup.connectivity.publicFailed");
    expect(card).toHaveTextContent("隧道不回包");
    expect(card).not.toHaveTextContent("cursorSetup.connectivity.localFailed");
    expect(
      screen.queryByTestId("cursor-setup-connectivity-localFailed"),
    ).not.toBeInTheDocument();
  });

  it("「重新测试」按 local → public 顺序重跑整条链路", async () => {
    const order: string[] = [];
    let firstRun = true;
    vi.mocked(proxyApi.testPublicRouteConnectivity).mockImplementation(
      async (scope) => {
        order.push(scope);
        if (firstRun && scope === "local") {
          return { ok: false, status: null, detail: "本地代理未启动" };
        }
        return { ok: true, status: 200, detail: "" };
      },
    );
    renderDialog();

    await screen.findByTestId("cursor-setup-connectivity-localFailed");
    firstRun = false;
    fireEvent.click(
      screen.getByRole("button", { name: "cursorSetup.connectivity.retest" }),
    );

    await screen.findByTestId("cursor-setup-connectivity-ok");
    expect(order).toEqual(["local", "local", "public"]);
  });

  it("两层都通过时提示 Cursor 可用，并写明密钥仍需手工粘一次", async () => {
    renderDialog();

    const ok = await screen.findByTestId("cursor-setup-connectivity-ok");
    expect(ok).toHaveTextContent("cursorSetup.connectivity.ok");
    expect(ok).toHaveTextContent("cursorSetup.connectivity.keyReminder");
  });
});
