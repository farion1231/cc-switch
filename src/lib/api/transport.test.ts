import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearRemoteBackendOverride,
  getRemoteBackendConfig,
  setRemoteBackendOverride,
  testRemoteBackendConnection,
} from "./transport";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const fetchMock = vi.fn();

describe("transport remote backend override", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
    window.history.replaceState({}, "", "/");
    delete window.__CC_SWITCH_BACKEND_URL__;
    delete window.__CC_SWITCH_BACKEND_TOKEN__;
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
  });

  it("persists a configured remote backend URL and token", () => {
    setRemoteBackendOverride({
      url: "http://linux-host:9990/",
      token: "secret",
    });

    expect(getRemoteBackendConfig()).toEqual({
      url: "http://linux-host:9990",
      token: "secret",
    });
  });

  it("supports clearing the configured remote backend", () => {
    setRemoteBackendOverride({
      url: "http://linux-host:9990",
      token: "secret",
    });

    clearRemoteBackendOverride();

    expect(getRemoteBackendConfig()).toBeNull();
  });

  it("stores backend query credentials in session storage and removes the token from the URL", () => {
    window.history.replaceState(
      {},
      "",
      "/?backend=http%3A%2F%2Flinux-host%3A9990&backendToken=secret&view=settings",
    );

    expect(getRemoteBackendConfig()).toEqual({
      url: "http://linux-host:9990",
      token: "secret",
    });
    expect(window.sessionStorage.getItem("cc-switch-backend-token")).toBe(
      "secret",
    );
    expect(window.location.search).toBe(
      "?backend=http%3A%2F%2Flinux-host%3A9990&view=settings",
    );
  });

  it("validates a remote backend before activating it", async () => {
    fetchMock.mockResolvedValueOnce({
      ok: true,
      text: () =>
        Promise.resolve(
          JSON.stringify({
            client: { shell: "browser", os: "unknown" },
            backend: {
              os: "linux",
              headless: true,
              remote: true,
              capabilities: {},
            },
            relation: { coLocated: false },
          }),
        ),
    });

    const runtime = await testRemoteBackendConnection({
      url: "http://linux-host:9990/",
      token: "secret",
    });

    expect(runtime.backend.os).toBe("linux");
    expect(fetchMock).toHaveBeenCalledWith(
      "http://linux-host:9990/__cc_switch_webui__/invoke",
      expect.objectContaining({
        method: "POST",
        headers: expect.objectContaining({
          "x-cc-switch-webui-token": "secret",
        }),
        body: JSON.stringify({ cmd: "get_runtime_info", args: {} }),
      }),
    );
  });
});
