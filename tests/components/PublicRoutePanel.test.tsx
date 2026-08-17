import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { toast } from "sonner";
import { PublicRoutePanel } from "@/components/proxy/PublicRoutePanel";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: { defaultValue?: string }) =>
      opts?.defaultValue ?? key,
  }),
}));

vi.mock("sonner", () => ({
  toast: Object.assign(vi.fn(), {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
  }),
}));

const state = {
  status: {
    enabled: false,
    apiKey: "ccsk-test",
    tunnelRunning: false,
    publicUrl: null as string | null,
    localUrl: "http://127.0.0.1:15721",
    tunnelError: null as string | null,
    currentProviderName: "ThirdParty",
    tunnelMode: "quick",
    namedTunnel: null as string | null,
    namedHostname: null as string | null,
  },
  enablePending: false,
};

const enableMock = vi.fn().mockResolvedValue({});
const disableMock = vi.fn().mockResolvedValue({});
const setConfigMock = vi.fn().mockResolvedValue({});
const regenerateMock = vi.fn().mockResolvedValue({ apiKey: "ccsk-new" });

vi.mock("@/lib/query/proxy", () => ({
  usePublicRouteStatus: () => ({ data: state.status }),
  useEnablePublicRoute: () => ({
    mutateAsync: enableMock,
    isPending: state.enablePending,
  }),
  useDisablePublicRoute: () => ({ mutateAsync: disableMock, isPending: false }),
  useSetPublicRouteTunnelConfig: () => ({
    mutateAsync: setConfigMock,
    isPending: false,
  }),
  useListNamedTunnels: () => ({ data: undefined }),
  useRegeneratePublicRouteApiKey: () => ({
    mutateAsync: regenerateMock,
    isPending: false,
  }),
}));

const writeTextMock = vi.fn().mockResolvedValue(undefined);
Object.assign(navigator, {
  clipboard: { writeText: writeTextMock },
});

describe("PublicRoutePanel（公网路由面板）", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    state.status.enabled = false;
    state.status.publicUrl = null;
    state.status.tunnelMode = "quick";
    state.status.namedTunnel = null;
    state.status.namedHostname = null;
  });

  it("隧道就绪后自动复制 Base URL 并提示去 Cursor 粘贴", async () => {
    state.status.enabled = true;
    state.status.publicUrl = "https://abc-def.trycloudflare.com";
    render(<PublicRoutePanel />);

    await waitFor(() =>
      expect(writeTextMock).toHaveBeenCalledWith(
        "https://abc-def.trycloudflare.com/cursor/v1",
      ),
    );
    expect(vi.mocked(toast.success)).toHaveBeenCalledWith(
      expect.stringContaining("Cursor"),
      expect.anything(),
    );
  });

  it("未启用或隧道未就绪时不自动复制", () => {
    render(<PublicRoutePanel />);

    expect(writeTextMock).not.toHaveBeenCalled();
  });

  it("提供快速隧道与固定域名两种模式选项", () => {
    render(<PublicRoutePanel />);

    expect(screen.getByText(/快速隧道/)).toBeInTheDocument();
    expect(screen.getByText(/固定域名/)).toBeInTheDocument();
  });

  it("选择固定域名模式时展示隧道名与域名输入及一次性配置引导", () => {
    state.status.tunnelMode = "named";
    render(<PublicRoutePanel />);

    expect(screen.getByText("隧道名")).toBeInTheDocument();
    expect(
      screen.getByPlaceholderText(/cc\.example\.com/),
    ).toBeInTheDocument();
    expect(screen.getByText(/cloudflared login/)).toBeInTheDocument();
    expect(screen.getByText(/cloudflared tunnel create/)).toBeInTheDocument();
  });

  it("保存命名隧道配置时调用 setPublicRouteTunnelConfig", async () => {
    state.status.tunnelMode = "named";
    render(<PublicRoutePanel />);

    fireEvent.change(screen.getByPlaceholderText(/my-tunnel/), {
      target: { value: "cc-switch" },
    });
    fireEvent.change(screen.getByPlaceholderText(/cc\.example\.com/), {
      target: { value: "cc.example.com" },
    });
    fireEvent.click(screen.getByText(/保存/));

    await waitFor(() =>
      expect(setConfigMock).toHaveBeenCalledWith({
        mode: "named",
        namedTunnel: "cc-switch",
        namedHostname: "cc.example.com",
      }),
    );
  });

  it("快速隧道模式下开启时持久化 quick 配置（避免后端跑旧命名隧道）", async () => {
    // 默认 tunnelMode = quick
    render(<PublicRoutePanel />);

    fireEvent.click(screen.getByRole("switch"));

    await waitFor(() =>
      expect(setConfigMock).toHaveBeenCalledWith({
        mode: "quick",
        namedTunnel: null,
        namedHostname: null,
      }),
    );
    await waitFor(() => expect(enableMock).toHaveBeenCalledTimes(1));
  });

  it("命名隧道模式下开启时持久化 named 配置后再启用", async () => {
    state.status.tunnelMode = "named";
    render(<PublicRoutePanel />);

    fireEvent.change(screen.getByPlaceholderText(/my-tunnel/), {
      target: { value: "cc-switch" },
    });
    fireEvent.change(screen.getByPlaceholderText(/cc\.example\.com/), {
      target: { value: "cc.example.com" },
    });
    fireEvent.click(screen.getByRole("switch"));

    await waitFor(() =>
      expect(setConfigMock).toHaveBeenCalledWith({
        mode: "named",
        namedTunnel: "cc-switch",
        namedHostname: "cc.example.com",
      }),
    );
    await waitFor(() => expect(enableMock).toHaveBeenCalledTimes(1));
  });

  it("不再宣称地址已自动写入 Cursor", () => {
    state.status.enabled = true;
    state.status.publicUrl = "https://abc-def.trycloudflare.com";
    render(<PublicRoutePanel />);

    expect(
      screen.queryByText(/已自动写入 Cursor/),
    ).not.toBeInTheDocument();
  });

  it("开关切换进行中展示加载指示（不再卡在灰色开关）", () => {
    state.enablePending = true;
    render(<PublicRoutePanel />);

    expect(screen.getByTestId("public-route-toggle-pending")).toBeInTheDocument();
  });

  it("点击重新生成密钥时调用后端并复制新密钥", async () => {
    state.status.enabled = true;
    state.status.publicUrl = "https://abc-def.trycloudflare.com";
    state.status.apiKey = "ccsk-old";
    render(<PublicRoutePanel />);

    fireEvent.click(screen.getByRole("button", { name: /重新生成/ }));

    await waitFor(() => expect(regenerateMock).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(writeTextMock).toHaveBeenCalledWith("ccsk-new"),
    );
    expect(vi.mocked(toast.success)).toHaveBeenCalledWith(
      expect.stringContaining("Cursor"),
      expect.anything(),
    );
  });

  it("剪贴板被拒时重新生成不报错，降级提示手动复制", async () => {
    state.status.enabled = true;
    state.status.publicUrl = "https://abc-def.trycloudflare.com";
    state.status.apiKey = "ccsk-old";
    // 挂载时 auto-copy 会先写一次剪贴板，regenerate 再写一次 → 各 reject 一次
    writeTextMock.mockRejectedValueOnce(new Error("NotAllowedError"));
    writeTextMock.mockRejectedValueOnce(new Error("NotAllowedError"));
    render(<PublicRoutePanel />);

    fireEvent.click(screen.getByRole("button", { name: /重新生成/ }));

    await waitFor(() => expect(regenerateMock).toHaveBeenCalledTimes(1));
    // 剪贴板被拒不应视为重新生成失败：仍弹成功 toast，引导手动复制
    await waitFor(() =>
      expect(vi.mocked(toast.success)).toHaveBeenCalledWith(
        expect.stringContaining("手动复制"),
        expect.anything(),
      ),
    );
  });

  it("隧道就绪后列出所有应用的路由路径供复制", () => {
    state.status.enabled = true;
    state.status.publicUrl = "https://abc-def.trycloudflare.com";
    render(<PublicRoutePanel />);

    // /cursor/v1 同时出现在路由列表与接入示例中
    expect(
      screen.getAllByText("https://abc-def.trycloudflare.com/cursor/v1")
        .length,
    ).toBeGreaterThanOrEqual(1);
    for (const path of [
      "/claude",
      "/codex/v1",
      "/gemini",
      "/grokbuild/v1",
      "/claude-desktop",
    ]) {
      expect(
        screen.getByText(`https://abc-def.trycloudflare.com${path}`),
      ).toBeInTheDocument();
    }
  });
});
