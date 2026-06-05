import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import i18n from "i18next";
import zh from "@/i18n/locales/zh.json";
import { ManagementApiPanel } from "@/components/settings/ManagementApiPanel";
import type { ManagementApiSettings } from "@/types";

const apiMocks = vi.hoisted(() => ({
  getManagementApiStatus: vi.fn(),
  listManagementApiTokens: vi.fn(),
  listManagementApiPairingSessions: vi.fn(),
  listManagementApiAuditLogs: vi.fn(),
  startManagementApi: vi.fn(),
  stopManagementApi: vi.fn(),
  restartManagementApi: vi.fn(),
  createManagementApiToken: vi.fn(),
  revokeManagementApiToken: vi.fn(),
  approveManagementApiPairing: vi.fn(),
  rejectManagementApiPairing: vi.fn(),
  clearManagementApiAuditLogs: vi.fn(),
  openExternal: vi.fn(),
}));

vi.mock("@/lib/api/settings", () => ({
  settingsApi: apiMocks,
}));

vi.mock("@/lib/clipboard", () => ({
  copyText: vi.fn().mockResolvedValue(undefined),
}));

const config: ManagementApiSettings = {
  enabled: true,
  listenAddress: "127.0.0.1",
  port: 15722,
  lanEnabled: false,
  allowedCidrs: [],
  corsOrigins: [],
  tlsEnabled: false,
  certificateFingerprint: null,
  pairingEnabled: true,
};

describe("ManagementApiPanel", () => {
  beforeEach(async () => {
    apiMocks.getManagementApiStatus.mockResolvedValue({
      enabled: true,
      running: true,
      address: "127.0.0.1",
      port: 15722,
      baseUrl: "http://127.0.0.1:15722/v1",
      lanEnabled: false,
      tlsEnabled: false,
      tokenCount: 1,
      startedAt: "2026-06-05T12:00:00Z",
    });
    apiMocks.listManagementApiTokens.mockResolvedValue([]);
    apiMocks.listManagementApiPairingSessions.mockResolvedValue([]);
    apiMocks.listManagementApiAuditLogs.mockResolvedValue([]);

    i18n.addResourceBundle("zh", "translation", zh, true, true);
    await i18n.changeLanguage("zh");
  });

  it("renders localized labels and request/response demos", async () => {
    const { container } = render(
      <ManagementApiPanel config={config} onChange={vi.fn()} />,
    );

    await waitFor(() => {
      expect(apiMocks.getManagementApiStatus).toHaveBeenCalled();
    });

    expect(await screen.findByText("管理 API")).toBeInTheDocument();
    expect(screen.getByText("运行中")).toBeInTheDocument();
    expect(screen.getByText("API 示例")).toBeInTheDocument();
    expect(screen.getByText("请求 demo")).toBeInTheDocument();
    expect(screen.getByText("返回 demo")).toBeInTheDocument();
    expect(container).toHaveTextContent(
      "http://127.0.0.1:15722/v1/apps/codex/providers",
    );
    expect(container).toHaveTextContent('"data": []');
    expect(container).toHaveTextContent('"pairingId": "pairing-id"');
  });
});
