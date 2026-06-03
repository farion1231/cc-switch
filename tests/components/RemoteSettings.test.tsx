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
    invokeMock.mockResolvedValue("ok");
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
    expect(screen.getByText("settings.remoteTailscaleEnabled")).toBeInTheDocument();
    expect(screen.getByText("settings.remotePort")).toBeInTheDocument();
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

  it("checks Tailscale availability before enabling", async () => {
    invokeMock.mockResolvedValue(false); // Tailscale not available

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

    const toggle = screen.getByRole("switch", {
      name: /settings\.remotetailscaleenabled/i,
    });
    fireEvent.click(toggle);

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("check_tailscale_available");
      expect(toastErrorMock).toHaveBeenCalledWith(
        "settings.remoteTailscaleNotAvailable"
      );
      // Should not call onChange since Tailscale is not available
      expect(mockOnChange).not.toHaveBeenCalled();
    });
  });

  it("disables Tailscale toggle when remote server is disabled", () => {
    const disabledSettings = { ...defaultSettings, remoteEnabled: false };

    render(
      <RemoteSettings settings={disabledSettings} onChange={mockOnChange} />,
      { wrapper: createWrapper() }
    );

    const toggle = screen.getByRole("switch", {
      name: /settings\.remotetailscaleenabled/i,
    });
    expect(toggle).toBeDisabled();
  });

  it("enables Tailscale toggle when remote server is enabled", () => {
    const enabledSettings = { ...defaultSettings, remoteEnabled: true };

    render(
      <RemoteSettings settings={enabledSettings} onChange={mockOnChange} />,
      { wrapper: createWrapper() }
    );

    const toggle = screen.getByRole("switch", {
      name: /settings\.remotetailscaleenabled/i,
    });
    expect(toggle).not.toBeDisabled();
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
