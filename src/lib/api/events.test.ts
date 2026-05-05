import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { listenBackendEvent } from "./events";
import { runtimeApi, type RuntimeInfo } from "./runtime";
import { getEffectiveRemoteBackendConfig } from "./transport";

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(),
}));

vi.mock("./runtime", () => ({
  runtimeApi: {
    getCached: vi.fn(),
  },
}));

vi.mock("./transport", () => ({
  getEffectiveRemoteBackendConfig: vi.fn(),
}));

const listenMock = vi.mocked(listen);
const getCachedMock = vi.mocked(runtimeApi.getCached);
const getEffectiveRemoteBackendConfigMock = vi.mocked(
  getEffectiveRemoteBackendConfig,
);
const fetchMock = vi.fn();

const runtimeInfo = (coLocated: boolean): RuntimeInfo => ({
  client: { shell: coLocated ? "desktop" : "browser", os: "windows" },
  backend: {
    os: "linux",
    headless: !coLocated,
    remote: !coLocated,
    capabilities: {
      readConfig: true,
      writeConfig: true,
      openLocalFolder: coLocated,
      pickDirectory: coLocated,
      serverDirectoryBrowse: true,
      appConfigDirOverride: coLocated,
      saveFileDialog: coLocated,
      openFileDialog: coLocated,
      launchInteractiveTerminal: coLocated,
      launchBackgroundProcess: coLocated,
      autoLaunch: coLocated,
      toolVersionCheck: true,
      windowControls: coLocated,
      dragRegion: coLocated,
      tray: coLocated,
    },
  },
  relation: { coLocated },
});

const streamFromText = (text: string): ReadableStream<Uint8Array> =>
  new ReadableStream({
    start(controller) {
      controller.enqueue(new TextEncoder().encode(text));
      controller.close();
    },
  });

const createOpenEventStream = () => {
  let controller: ReadableStreamDefaultController<Uint8Array>;
  const body = new ReadableStream<Uint8Array>({
    start(streamController) {
      controller = streamController;
    },
  });

  return {
    body,
    send(text: string) {
      controller.enqueue(new TextEncoder().encode(text));
    },
  };
};

describe("listenBackendEvent", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
    delete (window as unknown as { __CC_SWITCH_WEBUI__?: boolean })
      .__CC_SWITCH_WEBUI__;
    listenMock.mockReset();
    getCachedMock.mockReset();
    getEffectiveRemoteBackendConfigMock.mockReset();
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
  });

  it("uses Tauri events when the desktop frontend is colocated with the backend", async () => {
    const unlisten = vi.fn() as UnlistenFn;
    getCachedMock.mockResolvedValue(runtimeInfo(true));
    listenMock.mockResolvedValue(unlisten);

    const handler = vi.fn();
    const result = await listenBackendEvent("provider-switched", handler);

    expect(result).toBe(unlisten);
    expect(listenMock).toHaveBeenCalledWith("provider-switched", handler);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("subscribes to a remote backend SSE stream with the configured token", async () => {
    getCachedMock.mockResolvedValue(runtimeInfo(false));
    getEffectiveRemoteBackendConfigMock.mockResolvedValue({
      url: "http://linux-host:9990",
      token: "secret",
    });
    fetchMock.mockResolvedValue({
      ok: true,
      body: streamFromText(
        'data: {"event":"provider-switched","payload":{"appType":"claude","providerId":"p1"}}\n\n',
      ),
    });

    const handler = vi.fn();
    const unlisten = await listenBackendEvent("provider-switched", handler);

    await vi.waitFor(() => expect(handler).toHaveBeenCalledTimes(1));
    expect(handler).toHaveBeenCalledWith({
      event: "provider-switched",
      payload: { appType: "claude", providerId: "p1" },
    });
    expect(fetchMock).toHaveBeenCalledWith(
      "http://linux-host:9990/__cc_switch_webui__/events",
      expect.objectContaining({
        method: "GET",
        headers: expect.objectContaining({
          accept: "text/event-stream",
          "x-cc-switch-webui-token": "secret",
        }),
      }),
    );

    const request = fetchMock.mock.calls[0]?.[1] as RequestInit;
    unlisten();
    expect((request.signal as AbortSignal).aborted).toBe(true);
  });

  it("filters remote SSE messages by event name", async () => {
    getCachedMock.mockResolvedValue(runtimeInfo(false));
    getEffectiveRemoteBackendConfigMock.mockResolvedValue({
      url: "http://linux-host:9990",
    });
    fetchMock.mockResolvedValue({
      ok: true,
      body: streamFromText(
        [
          'data: {"event":"usage-cache-updated","payload":{"ignored":true}}',
          "",
          'data: {"event":"provider-switched","payload":{"appType":"codex","providerId":"p2"}}',
          "",
        ].join("\n"),
      ),
    });

    const handler = vi.fn();
    const unlisten = await listenBackendEvent("provider-switched", handler);

    await vi.waitFor(() => expect(handler).toHaveBeenCalledTimes(1));
    expect(handler).toHaveBeenCalledWith({
      event: "provider-switched",
      payload: { appType: "codex", providerId: "p2" },
    });
    unlisten();
  });

  it("shares one remote SSE fetch across different event subscriptions", async () => {
    getCachedMock.mockResolvedValue(runtimeInfo(false));
    getEffectiveRemoteBackendConfigMock.mockResolvedValue({
      url: "http://linux-host:9990",
    });
    const eventStream = createOpenEventStream();
    fetchMock.mockResolvedValue({
      ok: true,
      body: eventStream.body,
    });

    const providerHandler = vi.fn();
    const usageHandler = vi.fn();
    const providerUnlisten = await listenBackendEvent(
      "provider-switched",
      providerHandler,
    );
    const usageUnlisten = await listenBackendEvent(
      "usage-cache-updated",
      usageHandler,
    );

    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1));
    eventStream.send(
      [
        'data: {"event":"provider-switched","payload":{"providerId":"p1"}}',
        "",
        'data: {"event":"usage-cache-updated","payload":{"status":"ready"}}',
        "",
        "",
      ].join("\n"),
    );

    await vi.waitFor(() => expect(providerHandler).toHaveBeenCalledTimes(1));
    await vi.waitFor(() => expect(usageHandler).toHaveBeenCalledTimes(1));
    expect(providerHandler).toHaveBeenCalledWith({
      event: "provider-switched",
      payload: { providerId: "p1" },
    });
    expect(usageHandler).toHaveBeenCalledWith({
      event: "usage-cache-updated",
      payload: { status: "ready" },
    });

    providerUnlisten();
    usageUnlisten();
  });

  it("dispatches the same remote SSE event to multiple handlers", async () => {
    getCachedMock.mockResolvedValue(runtimeInfo(false));
    getEffectiveRemoteBackendConfigMock.mockResolvedValue({
      url: "http://linux-host:9990",
    });
    const eventStream = createOpenEventStream();
    fetchMock.mockResolvedValue({
      ok: true,
      body: eventStream.body,
    });

    const firstHandler = vi.fn();
    const secondHandler = vi.fn();
    const firstUnlisten = await listenBackendEvent(
      "provider-switched",
      firstHandler,
    );
    const secondUnlisten = await listenBackendEvent(
      "provider-switched",
      secondHandler,
    );

    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1));
    eventStream.send(
      'data: {"event":"provider-switched","payload":{"providerId":"p1"}}\n\n',
    );

    await vi.waitFor(() => expect(firstHandler).toHaveBeenCalledTimes(1));
    await vi.waitFor(() => expect(secondHandler).toHaveBeenCalledTimes(1));
    expect(firstHandler).toHaveBeenCalledWith({
      event: "provider-switched",
      payload: { providerId: "p1" },
    });
    expect(secondHandler).toHaveBeenCalledWith({
      event: "provider-switched",
      payload: { providerId: "p1" },
    });

    firstUnlisten();
    secondUnlisten();
  });

  it("aborts the shared remote SSE fetch after the last handler unlistens", async () => {
    getCachedMock.mockResolvedValue(runtimeInfo(false));
    getEffectiveRemoteBackendConfigMock.mockResolvedValue({
      url: "http://linux-host:9990",
    });
    const eventStream = createOpenEventStream();
    fetchMock.mockResolvedValue({
      ok: true,
      body: eventStream.body,
    });

    const firstUnlisten = await listenBackendEvent(
      "provider-switched",
      vi.fn(),
    );
    const secondUnlisten = await listenBackendEvent(
      "usage-cache-updated",
      vi.fn(),
    );

    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1));
    const request = fetchMock.mock.calls[0]?.[1] as RequestInit;

    firstUnlisten();
    expect((request.signal as AbortSignal).aborted).toBe(false);

    secondUnlisten();
    expect((request.signal as AbortSignal).aborted).toBe(true);
  });

  it("uses the same-origin WebUI event endpoint and session token in CLI WebUI", async () => {
    (
      window as unknown as { __CC_SWITCH_WEBUI__?: boolean }
    ).__CC_SWITCH_WEBUI__ = true;
    window.sessionStorage.setItem("cc-switch-webui-token", "session-secret");
    getCachedMock.mockResolvedValue(runtimeInfo(false));
    fetchMock.mockResolvedValue({
      ok: true,
      body: streamFromText(
        'data: {"event":"universal-provider-synced","payload":{"action":"sync","id":"u1"}}\n\n',
      ),
    });

    const handler = vi.fn();
    const unlisten = await listenBackendEvent(
      "universal-provider-synced",
      handler,
    );

    await vi.waitFor(() => expect(handler).toHaveBeenCalledTimes(1));
    expect(getEffectiveRemoteBackendConfigMock).not.toHaveBeenCalled();
    expect(fetchMock).toHaveBeenCalledWith(
      "/__cc_switch_webui__/events",
      expect.objectContaining({
        headers: expect.objectContaining({
          "x-cc-switch-webui-token": "session-secret",
        }),
      }),
    );
    unlisten();
  });
});
