import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DeepLinkImportDialog } from "@/components/DeepLinkImportDialog";
import type { DeepLinkImportRequest } from "@/lib/api/deeplink";

const mocks = vi.hoisted(() => ({
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
  getCurrentUrls: vi.fn(),
  parseDeeplink: vi.fn(),
  importFromDeeplink: vi.fn(),
  toastSuccess: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(
    (eventName: string, callback: (event: { payload: unknown }) => void) => {
      mocks.listeners.set(eventName, callback);
      return Promise.resolve(vi.fn());
    },
  ),
}));

vi.mock("@/lib/api/deeplink", () => ({
  deeplinkApi: {
    getCurrentUrls: mocks.getCurrentUrls,
    parseDeeplink: mocks.parseDeeplink,
    mergeDeeplinkConfig: vi.fn((request: DeepLinkImportRequest) => request),
    importFromDeeplink: mocks.importFromDeeplink,
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("sonner", () => ({
  toast: {
    success: mocks.toastSuccess,
    error: vi.fn(),
    warning: vi.fn(),
  },
}));

vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogContent: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogDescription: ({ children }: { children: React.ReactNode }) => (
    <p>{children}</p>
  ),
  DialogFooter: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogHeader: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  DialogTitle: ({ children }: { children: React.ReactNode }) => (
    <h1>{children}</h1>
  ),
}));

const request: DeepLinkImportRequest = {
  version: "v1",
  resource: "provider",
  app: "codex",
  name: "codex订阅",
  endpoint: "https://api.tu-zi.com/coding",
  apiKey: "sk-test-secret",
  model: "gpt-5.6-sol",
};

function renderDialog() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <DeepLinkImportDialog />
    </QueryClientProvider>,
  );
}

async function openDialog(payload: DeepLinkImportRequest = request) {
  renderDialog();
  await waitFor(() =>
    expect(mocks.listeners.has("deeplink-import")).toBe(true),
  );
  act(() => {
    mocks.listeners.get("deeplink-import")?.({ payload });
  });
}

describe("DeepLinkImportDialog provider editable fields", () => {
  beforeEach(() => {
    mocks.listeners.clear();
    mocks.getCurrentUrls.mockResolvedValue(null);
    mocks.parseDeeplink.mockResolvedValue(request);
    mocks.importFromDeeplink.mockResolvedValue({
      type: "provider",
      id: "provider-id",
    });
    mocks.toastSuccess.mockClear();
  });

  it("冷启动时会补取启动 Deep Link", async () => {
    mocks.getCurrentUrls.mockResolvedValue([
      "ccswitch://v1/import?resource=provider",
    ]);

    renderDialog();

    expect(await screen.findByLabelText("deeplink.providerName")).toHaveValue(
      "codex订阅",
    );
    expect(mocks.parseDeeplink).toHaveBeenCalledWith(
      "ccswitch://v1/import?resource=provider",
    );
  });

  it("不修改时按 Deep Link 默认值导入", async () => {
    await openDialog();

    fireEvent.click(screen.getByRole("button", { name: "deeplink.import" }));

    await waitFor(() =>
      expect(mocks.importFromDeeplink).toHaveBeenCalledOnce(),
    );
    expect(mocks.importFromDeeplink).toHaveBeenCalledWith(request);
  });

  it("仅覆盖用户修改的名称和模型，端点与密钥保持原值", async () => {
    await openDialog();

    fireEvent.change(screen.getByLabelText("deeplink.providerName"), {
      target: { value: "  我的供应商  " },
    });
    fireEvent.change(screen.getByLabelText("deeplink.model"), {
      target: { value: "  gpt-custom  " },
    });
    fireEvent.click(screen.getByRole("button", { name: "deeplink.import" }));

    await waitFor(() =>
      expect(mocks.importFromDeeplink).toHaveBeenCalledOnce(),
    );
    expect(mocks.importFromDeeplink).toHaveBeenCalledWith({
      ...request,
      name: "我的供应商",
      model: "gpt-custom",
    });
  });

  it("导入并启用 Codex 后提醒重启客户端", async () => {
    await openDialog({ ...request, enabled: true });

    fireEvent.click(screen.getByRole("button", { name: "deeplink.import" }));

    await waitFor(() =>
      expect(mocks.toastSuccess).toHaveBeenCalledWith(
        "deeplink.importSuccess",
        expect.objectContaining({
          description: "notifications.codexRestartRequired",
        }),
      ),
    );
  });

  it("供应商名称为空时禁止导入", async () => {
    await openDialog({ ...request, name: " " });

    expect(
      screen.getByRole("button", { name: "deeplink.import" }),
    ).toBeDisabled();
    expect(mocks.importFromDeeplink).not.toHaveBeenCalled();
  });
});
