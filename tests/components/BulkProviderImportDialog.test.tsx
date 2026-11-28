import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import zh from "@/i18n/locales/zh.json";
import { BulkProviderImportDialog } from "@/components/providers/BulkProviderImportDialog";
import {
  providersApi,
  type BulkProviderPreview,
  type BulkImportApp,
} from "@/lib/api/providers";
import { BULK_IMPORT_FORMATS } from "@/config/bulkImport";
import { toast } from "@/lib/toast";

vi.mock("@/lib/api/providers", () => ({
  providersApi: {
    previewBulkImport: vi.fn(),
    importBulk: vi.fn(),
    updateTrayMenu: vi.fn(),
  },
}));
vi.mock("@/lib/toast", () => ({ toast: { success: vi.fn() } }));

const source = JSON.stringify([
  {
    name: "Team",
    base_url: "https://gateway.example.com",
    api_key: "synthetic-private-key-1234",
  },
]);
function preview(): BulkProviderPreview {
  return {
    app: "claude",
    token: "review-token",
    newCount: 1,
    duplicateCount: 0,
    errors: [],
    rows: [
      {
        row: 1,
        name: "Team",
        baseUrl: "https://gateway.example.com",
        apiFormat: "anthropic",
        model: null,
        keyHint: "••••1234",
        requiresRouting: false,
        duplicate: false,
        sameName: false,
      },
    ],
  };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}
function mount(appId: BulkImportApp = "claude") {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const refresh = vi.spyOn(client, "invalidateQueries");
  const close = vi.fn();
  const navigation = vi.fn();
  const view = render(
    <QueryClientProvider client={client}>
      <BulkProviderImportDialog
        appId={appId}
        onClose={close}
        onNavigationBlockedChange={navigation}
      />
    </QueryClientProvider>,
  );
  return { ...view, close, refresh, navigation };
}
function input(text = source) {
  fireEvent.change(screen.getByLabelText("供应商 JSON"), {
    target: { value: text },
  });
}
async function review() {
  input();
  fireEvent.click(screen.getByRole("button", { name: "预览导入" }));
  await screen.findByText("••••1234");
}

beforeEach(async () => {
  i18n.addResourceBundle("zh", "translation", zh, true, true);
  await i18n.changeLanguage("zh");
  vi.mocked(providersApi.previewBulkImport)
    .mockReset()
    .mockResolvedValue(preview());
  vi.mocked(providersApi.importBulk)
    .mockReset()
    .mockResolvedValue({ addedIds: ["new-id"], skipped: 0 });
  vi.mocked(providersApi.updateTrayMenu).mockReset().mockResolvedValue(true);
});

describe("bulk provider import", () => {
  it("shows a non-blocking OMP key warning and imports the reviewed literal value", async () => {
    const raw = JSON.stringify([
      {
        name: "Team",
        base_url: "https://gateway.example.com",
        api_key: "abc123",
        model: "team-model",
      },
    ]);
    const result = preview();
    result.app = "omp";
    result.rows[0].keyHint = "••••••••";
    result.rows[0].keyMayResolveFromEnvironment = true;
    vi.mocked(providersApi.previewBulkImport).mockResolvedValue(result);
    const { close } = mount("omp");
    input(raw);
    fireEvent.click(screen.getByRole("button", { name: "预览导入" }));
    expect(
      await screen.findByRole("note", { name: "OMP 的 Key 解析规则" }),
    ).toHaveTextContent("只保存输入原值");
    expect(screen.getByText("可能读取环境变量")).toBeInTheDocument();
    expect(document.body.textContent).not.toContain("abc123");
    const confirm = screen.getByRole("button", { name: "导入 1 个供应商" });
    expect(confirm).toBeEnabled();
    expect(providersApi.importBulk).not.toHaveBeenCalled();
    fireEvent.click(confirm);
    await waitFor(() => expect(close).toHaveBeenCalledOnce());
    expect(providersApi.importBulk).toHaveBeenCalledWith(
      "omp",
      raw,
      "review-token",
    );
  });

  it("invalidates the OMP warning preview when the source changes", async () => {
    const result = preview();
    result.app = "omp";
    result.rows[0].keyMayResolveFromEnvironment = true;
    vi.mocked(providersApi.previewBulkImport).mockResolvedValue(result);
    mount("omp");
    await review();
    expect(screen.getByRole("note")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "修改 JSON" }));
    input("[]");
    expect(screen.queryByRole("note")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "导入 1 个供应商" }),
    ).not.toBeInTheDocument();
    expect(providersApi.importBulk).not.toHaveBeenCalled();
  });

  it("does not turn an OMP command validation error into a confirmable warning", async () => {
    const result = preview();
    result.app = "omp";
    result.token = null;
    result.rows[0].keyMayResolveFromEnvironment = true;
    result.errors = [
      { row: 2, field: "api_key", code: "dynamicValue", limit: null },
    ];
    vi.mocked(providersApi.previewBulkImport).mockResolvedValue(result);
    mount("omp");
    input();
    fireEvent.click(screen.getByRole("button", { name: "预览导入" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "不支持导入命令",
    );
    expect(screen.queryByRole("note")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "导入 1 个供应商" }),
    ).not.toBeInTheDocument();
    expect(providersApi.importBulk).not.toHaveBeenCalled();
  });
  it.each(Object.keys(BULK_IMPORT_FORMATS) as BulkImportApp[])(
    "keeps the %s target through preview and confirmation",
    async (app) => {
      const spec = BULK_IMPORT_FORMATS[app];
      const result = preview();
      result.app = app;
      result.rows[0].apiFormat = spec.defaultFormat;
      result.rows[0].keyMayResolveFromEnvironment = app === "omp";
      if (app === "kimi") result.rows[0].contextWindow = 32768;
      vi.mocked(providersApi.previewBulkImport).mockResolvedValue(result);
      const { close, refresh } = mount(app);
      const example = JSON.parse(
        screen.getByLabelText("供应商 JSON").getAttribute("placeholder")!,
      )[0];
      expect(example.api_format).toBe(spec.defaultFormat);
      expect(Boolean(example.model)).toBe(spec.modelRequired);
      if (app === "kimi") expect(example.context_window).toBeGreaterThan(0);
      await review();
      expect(screen.queryByRole("note") !== null).toBe(app === "omp");
      if (app === "kimi")
        expect(screen.getByText("上下文：32768 tokens")).toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "导入 1 个供应商" }));
      await waitFor(() => expect(close).toHaveBeenCalledOnce());
      expect(providersApi.importBulk).toHaveBeenCalledWith(
        app,
        source,
        "review-token",
      );
      expect(refresh).toHaveBeenCalledWith({ queryKey: ["providers", app] });
    },
  );
  it("previews masked credentials, commits the reviewed source and refreshes the captured app", async () => {
    const { close, refresh } = mount("codex");
    await review();
    expect(screen.queryByRole("note")).not.toBeInTheDocument();
    expect(providersApi.previewBulkImport).toHaveBeenCalledWith(
      "codex",
      source,
    );
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    expect(document.body.textContent).not.toContain("synthetic-private-key");
    fireEvent.click(screen.getByRole("button", { name: "导入 1 个供应商" }));
    await waitFor(() => expect(close).toHaveBeenCalledOnce());
    expect(providersApi.importBulk).toHaveBeenCalledWith(
      "codex",
      source,
      "review-token",
    );
    expect(refresh).toHaveBeenCalledWith({ queryKey: ["providers", "codex"] });
    expect(providersApi.updateTrayMenu).toHaveBeenCalledOnce();
  });

  it("blocks the entire batch on row validation errors", async () => {
    vi.mocked(providersApi.previewBulkImport).mockResolvedValue({
      ...preview(),
      token: null,
      errors: [{ row: 2, field: "api_key", code: "required", limit: null }],
    });
    mount();
    input();
    fireEvent.click(screen.getByRole("button", { name: "预览导入" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "第 2 行 · api_key: 必须填写非空文本",
    );
    expect(
      screen.queryByRole("button", { name: "导入 1 个供应商" }),
    ).not.toBeInTheDocument();
    expect(providersApi.importBulk).not.toHaveBeenCalled();
  });

  it("discards an outstanding preview when JSON changes", async () => {
    const pending = deferred<BulkProviderPreview>();
    vi.mocked(providersApi.previewBulkImport).mockReturnValueOnce(
      pending.promise,
    );
    mount();
    input();
    fireEvent.click(screen.getByRole("button", { name: "预览导入" }));
    input("[]");
    await act(async () => pending.resolve(preview()));
    expect(screen.getByLabelText("供应商 JSON")).toHaveValue("[]");
    expect(screen.queryByText("••••1234")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "预览导入" })).toBeEnabled();
  });

  it("requires a new preview after returning to edit", async () => {
    mount();
    await review();
    fireEvent.click(screen.getByRole("button", { name: "修改 JSON" }));
    expect(screen.getByLabelText("供应商 JSON")).toHaveValue(source);
    input("[]");
    expect(
      screen.queryByRole("button", { name: "导入 1 个供应商" }),
    ).not.toBeInTheDocument();
  });

  it("loads JSON files, enforces file size, and ignores late file reads", async () => {
    mount();
    const file = new File([source], "providers.json", {
      type: "application/json",
    });
    Object.defineProperty(file, "text", {
      value: () => Promise.resolve(source),
    });
    fireEvent.change(screen.getByLabelText("选择 JSON 文件"), {
      target: { files: [file] },
    });
    await waitFor(() =>
      expect(screen.getByLabelText("供应商 JSON")).toHaveValue(source),
    );
    const pending = deferred<string>();
    const slow = new File([], "slow.json");
    Object.defineProperty(slow, "text", { value: () => pending.promise });
    fireEvent.change(screen.getByLabelText("选择 JSON 文件"), {
      target: { files: [slow] },
    });
    input("[]");
    await act(async () => pending.resolve(source));
    expect(screen.getByLabelText("供应商 JSON")).toHaveValue("[]");
    const huge = new File([], "huge.json");
    Object.defineProperty(huge, "size", { value: 1024 * 1024 + 1 });
    fireEvent.change(screen.getByLabelText("选择 JSON 文件"), {
      target: { files: [huge] },
    });
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "JSON 不能超过 1 MiB",
    );
    expect(providersApi.previewBulkImport).not.toHaveBeenCalled();
  });

  it("clears stale previews and retains JSON after a concurrent provider change", async () => {
    vi.mocked(providersApi.importBulk).mockRejectedValue(
      "provider.bulkImport.errors.previewChanged",
    );
    const { close } = mount();
    await review();
    fireEvent.click(screen.getByRole("button", { name: "导入 1 个供应商" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "供应商列表已变化",
    );
    expect(screen.getByLabelText("供应商 JSON")).toHaveValue(source);
    expect(close).not.toHaveBeenCalled();
  });

  it("guards duplicate confirmation and prevents closing during commit", async () => {
    const pending = deferred<{ addedIds: string[]; skipped: number }>();
    vi.mocked(providersApi.importBulk).mockReturnValueOnce(pending.promise);
    const { close, navigation } = mount();
    await review();
    const button = screen.getByRole("button", { name: "导入 1 个供应商" });
    fireEvent.click(button);
    fireEvent.click(button);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(close).not.toHaveBeenCalled();
    expect(providersApi.importBulk).toHaveBeenCalledOnce();
    expect(button).toBeDisabled();
    expect(navigation).toHaveBeenLastCalledWith(true);
    await act(async () => pending.resolve({ addedIds: ["new"], skipped: 0 }));
    expect(close).toHaveBeenCalledOnce();
    expect(navigation).toHaveBeenLastCalledWith(false);
  });

  it("does not close a subsequent panel after unmounting during commit", async () => {
    const pending = deferred<{ addedIds: string[]; skipped: number }>();
    vi.mocked(providersApi.importBulk).mockReturnValueOnce(pending.promise);
    const { close, unmount, refresh } = mount();
    await review();
    fireEvent.click(screen.getByRole("button", { name: "导入 1 个供应商" }));
    unmount();
    await act(async () => pending.resolve({ addedIds: ["new"], skipped: 0 }));
    expect(close).not.toHaveBeenCalled();
    expect(refresh).toHaveBeenCalled();
  });

  it("does not report a committed import as failed when refreshing the tray fails", async () => {
    vi.mocked(providersApi.updateTrayMenu).mockRejectedValue(
      new Error("tray unavailable"),
    );
    const { close, refresh } = mount();
    refresh.mockRejectedValueOnce(new Error("refresh unavailable"));
    await review();
    fireEvent.click(screen.getByRole("button", { name: "导入 1 个供应商" }));
    await waitFor(() => expect(close).toHaveBeenCalledOnce());
    expect(toast.success).toHaveBeenCalled();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("shows duplicate, same-name and routing decisions and blocks a no-op batch", async () => {
    const result = preview();
    result.rows[0] = {
      ...result.rows[0],
      duplicate: true,
      requiresRouting: true,
    };
    result.rows.push({
      ...result.rows[0],
      row: 2,
      duplicate: false,
      sameName: true,
      name: "Team B",
    });
    result.newCount = 0;
    result.duplicateCount = 2;
    vi.mocked(providersApi.previewBulkImport).mockResolvedValue(result);
    mount();
    input();
    fireEvent.click(screen.getByRole("button", { name: "预览导入" }));
    expect(await screen.findByText("跳过重复")).toBeInTheDocument();
    expect(screen.getByText("新增 · 同名保留")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "导入 0 个供应商" }),
    ).toBeDisabled();
  });

  it("does not expose unexpected backend error details", async () => {
    vi.mocked(providersApi.importBulk).mockRejectedValue(
      "raw-secret-from-storage",
    );
    mount();
    await review();
    fireEvent.click(screen.getByRole("button", { name: "导入 1 个供应商" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("操作未能完成");
    expect(document.body.textContent).not.toContain("raw-secret-from-storage");
  });
});
