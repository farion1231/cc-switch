import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { GlobalProxySettings } from "@/components/settings/GlobalProxySettings";
import type { UpstreamProxyStatus } from "@/lib/api/globalProxy";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const mutateAsyncMock = vi.fn();
const testMutateAsyncMock = vi.fn();
const scanMutateAsyncMock = vi.fn();
const followMutateMock = vi.fn();
const refetchStatusMock = vi.fn();

const state = vi.hoisted(() => ({
  savedUrl: "http://127.0.0.1:7890" as string | null,
  followSystemProxy: true as boolean | undefined,
  status: null as unknown,
}));

vi.mock("@/hooks/useGlobalProxy", () => ({
  useGlobalProxyUrl: () => ({ data: state.savedUrl, isLoading: false }),
  useSetGlobalProxyUrl: () => ({
    mutateAsync: mutateAsyncMock,
    isPending: false,
  }),
  useTestProxy: () => ({
    mutateAsync: testMutateAsyncMock,
    isPending: false,
  }),
  useScanProxies: () => ({
    mutateAsync: scanMutateAsyncMock,
    isPending: false,
  }),
  useFollowSystemProxy: () => ({
    data: state.followSystemProxy,
    isLoading: false,
  }),
  useSetFollowSystemProxy: () => ({
    mutate: followMutateMock,
    isPending: false,
  }),
  useUpstreamProxyStatus: () => ({
    data: state.status,
    refetch: refetchStatusMock,
    isFetching: false,
  }),
}));

const systemStatus = (
  overrides: Partial<UpstreamProxyStatus> = {},
): UpstreamProxyStatus => ({
  enabled: false,
  proxyUrl: null,
  followSystemProxy: true,
  mode: "system",
  systemProxyUrl: "http://127.0.0.1:7890",
  systemProxySource: "env",
  systemProxyReachable: false,
  systemProxyChanged: false,
  currentSystemProxyUrl: "http://127.0.0.1:7890",
  ...overrides,
});

describe("GlobalProxySettings", () => {
  beforeEach(() => {
    mutateAsyncMock.mockReset();
    testMutateAsyncMock.mockReset();
    scanMutateAsyncMock.mockReset();
    followMutateMock.mockReset();
    refetchStatusMock.mockReset();
    state.savedUrl = "http://127.0.0.1:7890";
    state.followSystemProxy = true;
    state.status = null;
  });

  it("renders proxy URL input with saved value", async () => {
    render(<GlobalProxySettings />);

    const urlInput = screen.getByPlaceholderText(
      "http://127.0.0.1:7890 / socks5://127.0.0.1:1080",
    );
    // URL 对象会在末尾添加斜杠
    await waitFor(() => expect(urlInput).toHaveValue("http://127.0.0.1:7890/"));
  });

  it("saves proxy URL when save button is clicked", async () => {
    render(<GlobalProxySettings />);

    const urlInput = screen.getByPlaceholderText(
      "http://127.0.0.1:7890 / socks5://127.0.0.1:1080",
    );

    fireEvent.change(urlInput, { target: { value: "http://localhost:8080" } });

    const saveButton = screen.getByRole("button", { name: "common.save" });
    fireEvent.click(saveButton);

    await waitFor(() => expect(mutateAsyncMock).toHaveBeenCalled());
    // 没有用户名时，URL 不经过 URL 对象解析，所以没有尾部斜杠
    expect(mutateAsyncMock).toHaveBeenCalledWith("http://localhost:8080");
  });

  it("clears proxy URL when clear button is clicked", async () => {
    render(<GlobalProxySettings />);

    const urlInput = screen.getByPlaceholderText(
      "http://127.0.0.1:7890 / socks5://127.0.0.1:1080",
    );

    // Wait for initial value to load
    await waitFor(() => expect(urlInput).toHaveValue("http://127.0.0.1:7890/"));

    // Click clear button
    const clearButton = screen.getByTitle("settings.globalProxy.clear");
    fireEvent.click(clearButton);

    expect(urlInput).toHaveValue("");
  });

  it("disables the follow switch and explains why when a proxy URL is saved", () => {
    render(<GlobalProxySettings />);

    const toggle = screen.getByRole("switch", {
      name: "settings.globalProxy.followSystemProxy",
    });
    expect(toggle).toBeDisabled();
    expect(
      screen.getByText("settings.globalProxy.followSystemProxyExplicitHint"),
    ).toBeInTheDocument();
  });

  it("keeps the follow switch disabled until the setting has loaded", () => {
    state.savedUrl = null;
    state.followSystemProxy = undefined;
    render(<GlobalProxySettings />);

    expect(
      screen.getByRole("switch", {
        name: "settings.globalProxy.followSystemProxy",
      }),
    ).toBeDisabled();
  });

  it("toggles follow-system-proxy when no proxy URL is saved", () => {
    state.savedUrl = null;
    render(<GlobalProxySettings />);

    const toggle = screen.getByRole("switch", {
      name: "settings.globalProxy.followSystemProxy",
    });
    expect(toggle).toBeEnabled();
    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(
      screen.getByText("settings.globalProxy.followSystemProxyDescription"),
    ).toBeInTheDocument();

    fireEvent.click(toggle);
    expect(followMutateMock).toHaveBeenCalledWith(false);
  });

  it("shows the baked system proxy with reachability and source", () => {
    state.savedUrl = null;
    state.status = systemStatus();
    render(<GlobalProxySettings />);

    expect(
      screen.getByText("settings.globalProxy.status.system"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("settings.globalProxy.status.unreachable"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("settings.globalProxy.status.sourceEnv"),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", {
        name: "settings.globalProxy.status.reapply",
      }),
    ).not.toBeInTheDocument();
  });

  it("offers re-apply when the system proxy changed after the client was built", () => {
    state.savedUrl = null;
    state.status = systemStatus({
      systemProxyChanged: true,
      currentSystemProxyUrl: null,
    });
    render(<GlobalProxySettings />);

    expect(
      screen.getByText("settings.globalProxy.status.changedToNone"),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.globalProxy.status.reapply",
      }),
    );
    expect(followMutateMock).toHaveBeenCalledWith(true);
  });

  it("explains a change that keeps the displayed address", () => {
    state.savedUrl = null;
    state.status = systemStatus({ systemProxyChanged: true });
    render(<GlobalProxySettings />);

    expect(
      screen.getByText("settings.globalProxy.status.changedSameUrl"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: "settings.globalProxy.status.reapply",
      }),
    ).toBeInTheDocument();
  });

  it("states the local-routing scope only in system mode", () => {
    state.savedUrl = null;
    state.status = systemStatus();
    render(<GlobalProxySettings />);

    expect(
      screen.getByText("settings.globalProxy.status.systemScope"),
    ).toBeInTheDocument();
  });

  it("omits the local-routing scope note in direct mode", () => {
    state.savedUrl = null;
    state.followSystemProxy = false;
    state.status = systemStatus({
      followSystemProxy: false,
      mode: "direct",
      systemProxyUrl: null,
      systemProxySource: null,
      systemProxyReachable: null,
      currentSystemProxyUrl: null,
    });
    render(<GlobalProxySettings />);

    expect(
      screen.queryByText("settings.globalProxy.status.systemScope"),
    ).not.toBeInTheDocument();
  });

  it("renders no status block before the status has loaded", () => {
    render(<GlobalProxySettings />);

    expect(
      screen.queryByText("settings.globalProxy.status.label"),
    ).not.toBeInTheDocument();
  });

  it("shows the custom proxy line without system details in explicit mode", () => {
    state.status = systemStatus({
      enabled: true,
      proxyUrl: "http://127.0.0.1:7890",
      mode: "explicit",
      systemProxyUrl: null,
      systemProxySource: null,
      systemProxyReachable: null,
      currentSystemProxyUrl: null,
    });
    render(<GlobalProxySettings />);

    expect(
      screen.getByText("settings.globalProxy.status.explicit"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("settings.globalProxy.status.sourceEnv"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText("settings.globalProxy.status.unreachable"),
    ).not.toBeInTheDocument();
  });

  it("refreshes the effective status on demand", () => {
    state.savedUrl = null;
    state.status = systemStatus();
    render(<GlobalProxySettings />);

    fireEvent.click(screen.getByRole("button", { name: "common.refresh" }));
    expect(refetchStatusMock).toHaveBeenCalledTimes(1);
  });

  it("shows direct mode when following is off", () => {
    state.savedUrl = null;
    state.followSystemProxy = false;
    state.status = systemStatus({
      followSystemProxy: false,
      mode: "direct",
      systemProxyUrl: null,
      systemProxySource: null,
      systemProxyReachable: null,
      currentSystemProxyUrl: null,
    });
    render(<GlobalProxySettings />);

    expect(
      screen.getByText("settings.globalProxy.status.direct"),
    ).toBeInTheDocument();
    // 直连时状态来自应用自身的内存状态，重新探测没有意义，不给刷新入口
    expect(
      screen.queryByRole("button", { name: "common.refresh" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("switch", {
        name: "settings.globalProxy.followSystemProxy",
      }),
    ).toHaveAttribute("aria-checked", "false");
  });
});
