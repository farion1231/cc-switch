import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import "@testing-library/jest-dom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { http, HttpResponse } from "msw";
import { server } from "../msw/server";
import { RemoteSettings } from "@/components/settings/RemoteSettings";
import type { SettingsFormState } from "@/hooks/useSettings";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

const tMock = vi.fn((key: string) => key);
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: tMock }),
}));

// Mock Tauri invoke
const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

// Mock fetch for health check
const fetchMock = vi.fn();
global.fetch = fetchMock as any;

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });

  return ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
}

interface RemoteConfigFixture {
  bindScope: "loopback" | "tailscale" | "allInterfaces";
}

/** Default invoke responder: the component reads the scope from the backend. */
function mockBackend(scope: RemoteConfigFixture["bindScope"] = "loopback") {
  invokeMock.mockImplementation((command: string) => {
    switch (command) {
      case "get_remote_config":
        return Promise.resolve({
          enabled: true,
          port: 4000,
          tailscaleEnabled: scope !== "loopback",
          bindScope: scope,
          corsMode: "sameOrigin",
          originWhitelist: [],
        });
      case "get_tailscale_ip":
        return Promise.resolve("100.64.0.9");
      case "check_tailscale_available":
        return Promise.resolve(true);
      case "list_network_addresses":
        // 暴露面展示用的网卡列表；这些用例不关心具体地址，给空数组即可。
        return Promise.resolve([]);
      case "request_bind_scope_change":
        return Promise.resolve({ applied: true, bindScope: scope });
      default:
        return Promise.resolve("ok");
    }
  });
}

function scopeRadio(
  name: RegExp,
  options?: Parameters<typeof screen.getByRole>[1],
) {
  return screen.getByRole("radio", { name, ...(options ?? {}) });
}

describe("RemoteSettings", () => {
  const mockOnChange = vi.fn();
  const defaultSettings: SettingsFormState = {
    remoteEnabled: false,
    remotePort: 4000,
    remoteTailscaleEnabled: false,
    showInTray: true,
    minimizeToTrayOnClose: true,
    language: "zh",
  };

  beforeEach(() => {
    vi.clearAllMocks();
    mockBackend("loopback");
    fetchMock.mockResolvedValue({ ok: true });
    server.use(
      http.get(/http:\/\/127\.0\.0\.1:\d+\/api\/health/, () =>
        HttpResponse.json({ status: "ok" }),
      ),
    );
  });

  it("renders remote management section", () => {
    render(
      <RemoteSettings settings={defaultSettings} onChange={mockOnChange} />,
      { wrapper: createWrapper() }
    );

    expect(screen.getByText("settings.remoteManagement")).toBeInTheDocument();
    expect(screen.getByText("settings.remoteEnabled")).toBeInTheDocument();
    expect(screen.getByText("settings.remotePort")).toBeInTheDocument();
    // 三选一：旧 Tailscale 开关被档位选择取代。
    expect(screen.getByText("remote.settings.bindScope.loopback")).toBeInTheDocument();
    expect(screen.getByText("remote.settings.bindScope.tailscale")).toBeInTheDocument();
    expect(
      screen.getByText("remote.settings.bindScope.allInterfaces")
    ).toBeInTheDocument();
    expect(screen.queryByText("settings.remoteTailscaleEnabled")).toBeNull();
  });

  it("starts remote server when enabled toggle is turned on", async () => {
    render(
      <RemoteSettings settings={defaultSettings} onChange={mockOnChange} />,
      { wrapper: createWrapper() }
    );

    const toggle = screen.getByRole("switch", {
      name: /settings\.remoteenabled/i,
    });
    fireEvent.click(toggle);

    await waitFor(
      () => {
        expect(invokeMock).toHaveBeenCalledWith(
          "start_remote_server",
          expect.any(Object),
        );
        expect(mockOnChange).toHaveBeenCalledWith({ remoteEnabled: true });
      },
      { timeout: 3000 },
    );
  });

  it("stops remote server when enabled toggle is turned off", async () => {
    const enabledSettings = { ...defaultSettings, remoteEnabled: true };

    render(
      <RemoteSettings settings={enabledSettings} onChange={mockOnChange} />,
      { wrapper: createWrapper() }
    );

    const toggle = screen.getByRole("switch", {
      name: /settings\.remoteenabled/i,
    });
    fireEvent.click(toggle);

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("stop_remote_server");
      expect(mockOnChange).toHaveBeenCalledWith({ remoteEnabled: false });
    });
  });

  it("checks Tailscale availability before widening to Tailscale", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_remote_config") {
        return Promise.resolve({
          enabled: true,
          port: 4000,
          tailscaleEnabled: false,
          bindScope: "loopback",
          corsMode: "sameOrigin",
          originWhitelist: [],
        });
      }
      if (command === "get_tailscale_ip") return Promise.resolve(null);
      if (command === "check_tailscale_available") return Promise.resolve(false);
      if (command === "list_network_addresses") return Promise.resolve([]);
      return Promise.resolve("ok");
    });

    const settingsWithServerEnabled = {
      ...defaultSettings,
      remoteEnabled: true,
    };

    render(
      <RemoteSettings
        settings={settingsWithServerEnabled}
        onChange={mockOnChange}
      />,
      { wrapper: createWrapper() }
    );

    await waitFor(() =>
      expect(
        scopeRadio(/remote\.settings\.bindscope\.loopback/i)
      ).toBeChecked(),
    );

    fireEvent.click(scopeRadio(/remote\.settings\.bindscope\.tailscale/i));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("check_tailscale_available");
      expect(toastErrorMock).toHaveBeenCalledWith(
        "remote.settings.bindScope.tailscaleUnavailable"
      );
      // 不可用时不展示确认面板、也不发申请。
      expect(
        invokeMock
      ).not.toHaveBeenCalledWith("request_bind_scope_change", expect.anything());
      expect(mockOnChange).not.toHaveBeenCalled();
    });
  });

  it("disables bind scope options when remote server is disabled", () => {
    const disabledSettings = { ...defaultSettings, remoteEnabled: false };

    render(
      <RemoteSettings settings={disabledSettings} onChange={mockOnChange} />,
      { wrapper: createWrapper() }
    );

    expect(scopeRadio(/remote\.settings\.bindscope\.loopback/i)).toBeDisabled();
    expect(scopeRadio(/remote\.settings\.bindscope\.tailscale/i)).toBeDisabled();
    expect(
      scopeRadio(/remote\.settings\.bindscope\.allinterfaces/i)
    ).toBeDisabled();
  });

  it("enables bind scope options when remote server is enabled", async () => {
    const enabledSettings = { ...defaultSettings, remoteEnabled: true };

    render(
      <RemoteSettings settings={enabledSettings} onChange={mockOnChange} />,
      { wrapper: createWrapper() }
    );

    const radio = scopeRadio(/remote\.settings\.bindscope\.tailscale/i);
    expect(radio).not.toBeDisabled();
  });

  it("shows the confirmation panel for a widening change and applies it", async () => {
    mockBackend("loopback");
    const enabledSettings = { ...defaultSettings, remoteEnabled: true };

    render(
      <RemoteSettings settings={enabledSettings} onChange={mockOnChange} />,
      { wrapper: createWrapper() }
    );

    await waitFor(() =>
      expect(
        scopeRadio(/remote\.settings\.bindscope\.loopback/i)
      ).toBeChecked(),
    );

    // 选择更宽的档位：先出面板，**不**发申请。
    fireEvent.click(scopeRadio(/remote\.settings\.bindscope\.allinterfaces/i));
    const dialog = await screen.findByRole("dialog", {
      name: "remote.settings.bindScope.widenTitle",
    });
    expect(dialog).toBeInTheDocument();
    expect(
      invokeMock
    ).not.toHaveBeenCalledWith("request_bind_scope_change", expect.anything());

    // 面板里确认 → 调用 Tauri 命令（后端再弹原生对话框）。
    invokeMock.mockImplementation((command: string) => {
      if (command === "get_remote_config") {
        return Promise.resolve({
          enabled: true,
          port: 4000,
          tailscaleEnabled: false,
          bindScope: "loopback",
          corsMode: "sameOrigin",
          originWhitelist: [],
        });
      }
      if (command === "check_tailscale_available") return Promise.resolve(true);
      if (command === "get_tailscale_ip") return Promise.resolve("100.64.0.9");
      if (command === "request_bind_scope_change") {
        return Promise.resolve({ applied: true, bindScope: "allInterfaces" });
      }
      if (command === "list_network_addresses") return Promise.resolve([]);
      return Promise.resolve("ok");
    });

    fireEvent.click(
      screen.getByRole("button", {
        name: "remote.settings.bindScope.confirm",
      })
    );

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("request_bind_scope_change", {
        scope: "allInterfaces",
      });
      expect(mockOnChange).toHaveBeenCalledWith({
        remoteTailscaleEnabled: true,
      });
    });
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).toBeNull()
    );
  });

  it("applies a narrowing change immediately without the confirmation panel", async () => {
    mockBackend("allInterfaces");
    const enabledSettings = { ...defaultSettings, remoteEnabled: true };

    render(
      <RemoteSettings settings={enabledSettings} onChange={mockOnChange} />,
      { wrapper: createWrapper() }
    );

    await waitFor(() =>
      expect(
        scopeRadio(/remote\.settings\.bindscope\.allinterfaces/i)
      ).toBeChecked(),
    );

    invokeMock.mockImplementation((command: string) => {
      if (command === "get_remote_config") {
        return Promise.resolve({
          enabled: true,
          port: 4000,
          tailscaleEnabled: true,
          bindScope: "allInterfaces",
          corsMode: "sameOrigin",
          originWhitelist: [],
        });
      }
      if (command === "get_tailscale_ip") return Promise.resolve("100.64.0.9");
      if (command === "request_bind_scope_change") {
        return Promise.resolve({ applied: true, bindScope: "loopback" });
      }
      if (command === "list_network_addresses") return Promise.resolve([]);
      return Promise.resolve("ok");
    });

    fireEvent.click(scopeRadio(/remote\.settings\.bindscope\.loopback/i));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("request_bind_scope_change", {
        scope: "loopback",
      });
      expect(mockOnChange).toHaveBeenCalledWith({
        remoteTailscaleEnabled: false,
      });
    });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("allows port changes when server is not running", async () => {
    fetchMock.mockRejectedValue(new Error("Server not running"));

    const { container } = render(
      <RemoteSettings settings={defaultSettings} onChange={mockOnChange} />,
      { wrapper: createWrapper() }
    );

    // Find input by type and value
    const portInput = container.querySelector('input[type="number"]') as HTMLInputElement;
    expect(portInput).toBeInTheDocument();

    fireEvent.change(portInput, { target: { value: "4001" } });

    await waitFor(() => {
      expect(mockOnChange).toHaveBeenCalledWith({ remotePort: 4001 });
      // Should not try to stop/start server
      expect(invokeMock).not.toHaveBeenCalledWith("stop_remote_server");
      expect(invokeMock).not.toHaveBeenCalledWith(
        "start_remote_server",
        expect.any(Object),
      );
    });
  });

  it("handles server start errors gracefully", async () => {
    invokeMock.mockRejectedValue(new Error("Start failed"));

    render(
      <RemoteSettings settings={defaultSettings} onChange={mockOnChange} />,
      { wrapper: createWrapper() }
    );

    const toggle = screen.getByRole("switch", {
      name: /settings\.remoteenabled/i,
    });
    fireEvent.click(toggle);

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        "start_remote_server",
        expect.any(Object),
      );
      expect(toastErrorMock).toHaveBeenCalled();
    });
  });
});
