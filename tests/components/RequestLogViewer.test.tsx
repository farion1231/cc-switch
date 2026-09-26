import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { EditorView } from "@codemirror/view";
import { foldState } from "@codemirror/language";
import {
  afterAll,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";

import { RequestLogViewer } from "@/components/proxy/RequestLogViewer";
import type {
  ProxyRequestLogFileMeta,
  ProxyRequestLogListRow,
} from "@/types/proxy";

const { listFiles, getRecords, getRecord, copyTextMock } = vi.hoisted(() => ({
  listFiles: vi.fn(),
  getRecords: vi.fn(),
  getRecord: vi.fn(),
  copyTextMock: vi.fn(),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en" },
  }),
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

vi.mock("@/lib/api/proxy", () => ({
  proxyApi: {
    listProxyRequestLogFiles: listFiles,
    getProxyRequestLogRecords: getRecords,
    getProxyRequestLogRecord: getRecord,
    openProxyRequestLogDir: vi.fn(),
  },
}));

vi.mock("@/lib/clipboard", () => ({
  copyText: copyTextMock,
}));

const files: ProxyRequestLogFileMeta[] = [
  {
    appType: "claude",
    fileName: "session-a.jsonl",
    sizeBytes: 2048,
    modifiedAtMs: 1_700_000_000_000,
    sessionTitle: "Session A",
  },
  {
    appType: "claude",
    fileName: "session-b.jsonl",
    sizeBytes: 1024,
    modifiedAtMs: 1_700_000_100_000,
    sessionTitle: "Session B",
  },
];

function makeRecord(
  lineNo: number,
  overrides: Record<string, unknown> = {},
): Record<string, unknown> {
  return {
    lineNo,
    startTime: "2026-09-20T10:00:00.000Z",
    endTime: "2026-09-20T10:00:01.500Z",
    requestId: "req-12345678",
    method: "POST",
    endpoint: "/v1/messages",
    url: "https://api.anthropic.com/v1/messages",
    model: "claude-sonnet-5",
    durationMs: 1500,
    isStreaming: false,
    statusCode: 200,
    requestHeaders: [
      { name: "host", value: "api.anthropic.com" },
      { name: "content-type", value: "application/json" },
      { name: "content-length", value: "123" },
      { name: "authorization", value: "Bearer sk-test" },
    ],
    requestBody: {
      model: "claude-sonnet-5",
      messages: [{ role: "user", content: "hi" }],
    },
    responseHeaders: [{ name: "content-type", value: "application/json" }],
    responseBody: { id: "msg_01", content: [{ type: "text", text: "hello" }] },
    ...overrides,
  };
}

/** 列表轻量行：getRecords 返回（后端从完整记录提取） */
function makeListRow(
  lineNo: number,
  overrides: Partial<ProxyRequestLogListRow> = {},
): ProxyRequestLogListRow {
  return {
    lineNo,
    startTime: "2026-09-20T10:00:00.000Z",
    endTime: "2026-09-20T10:00:01.500Z",
    method: "POST",
    endpoint: "/v1/messages",
    model: "claude-sonnet-5",
    durationMs: 1500,
    isStreaming: false,
    statusCode: 200,
    promptTokens: null,
    completionTokens: null,
    error: null,
    ...overrides,
  };
}

const page = (records: ProxyRequestLogListRow[], total: number) => ({
  records,
  total,
});

function renderViewer() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <RequestLogViewer />
    </QueryClientProvider>,
  );
}

/** 展开第一条记录的详情行（等待完整记录按 lineNo 拉取并渲染） */
async function expandFirstRecord() {
  const rows = await screen.findAllByRole("button", { expanded: false });
  fireEvent.click(rows[0]);
  await screen.findByText("proxy.requestLogViewer.copyAsCurl");
  return rows[0];
}

/** 记录列表的滚动容器（sticky 表头的父级） */
function getScrollContainer(): HTMLElement {
  const sticky = document.querySelector("div.sticky");
  expect(sticky).not.toBeNull();
  return sticky!.parentElement as HTMLElement;
}

// jsdom 无布局：offsetHeight 恒为 0 会让虚拟列表认为视口高度为 0
// （virtual-core 的 getRect 读 offsetHeight），一行都不渲染。
// 给出非零高度让 calculateRange 正常计算。仅影响本文件（vitest 按文件隔离）。
const originalOffsetHeight = Object.getOwnPropertyDescriptor(
  HTMLElement.prototype,
  "offsetHeight",
);

beforeAll(() => {
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
    configurable: true,
    get() {
      return 600;
    },
  });
});

afterAll(() => {
  if (originalOffsetHeight) {
    Object.defineProperty(
      HTMLElement.prototype,
      "offsetHeight",
      originalOffsetHeight,
    );
  }
});

describe("RequestLogViewer", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    listFiles.mockResolvedValue(files);
    getRecords.mockResolvedValue(page([makeListRow(3), makeListRow(2)], 2));
    // 展开详情按 lineNo 取整条记录
    getRecord.mockImplementation(async (params: { lineNo: number }) =>
      makeRecord(params.lineNo),
    );
    copyTextMock.mockResolvedValue(undefined);
  });

  it("renders record rows and expands the detail view", async () => {
    renderViewer();

    expect(await screen.findAllByText("/v1/messages")).toHaveLength(2);

    fireEvent.click(screen.getAllByRole("button", { expanded: false })[0]);

    await screen.findByText("proxy.requestLogViewer.copyAsCurl");
    expect(
      screen.getByText("proxy.requestLogViewer.requestHeaders"),
    ).toBeDefined();
    expect(
      screen.getByText("proxy.requestLogViewer.responseBody"),
    ).toBeDefined();
    expect(screen.getByRole("button", { expanded: true })).toBeDefined();
  });

  it("places session identity in the record panel header, not the toolbar", async () => {
    // 会话标题是身份信息 → 右栏 Card 头部（h2）；文件名/ID 与大小作为元信息行；
    // 左栏是独立的「会话日志」列表卡片
    renderViewer();
    await screen.findAllByText("/v1/messages");

    const heading = screen.getByRole("heading", { level: 2 });
    expect(heading.textContent).toBe("Session A");
    expect(
      screen.getByText("proxy.requestLogViewer.sessionFiles"),
    ).toBeDefined();
    // 右栏头部元信息行：记录计数（左栏无此文案）
    expect(screen.getByText("proxy.requestLogViewer.shownOf")).toBeDefined();
  });

  it("shows the empty state when no log files exist", async () => {
    listFiles.mockResolvedValue([]);
    renderViewer();

    await screen.findByText("proxy.requestLogViewer.empty");
    expect(getRecords).not.toHaveBeenCalled();
  });

  it("extracts token usage per row across upstream formats", async () => {
    // token 数由后端从各上游形态提取（usage.input_tokens / prompt_tokens /
    // usageMetadata.promptTokenCount），列表行直接携带
    getRecords.mockResolvedValue(
      page(
        [
          makeListRow(1, { promptTokens: 1234, completionTokens: 567 }),
          makeListRow(2, { promptTokens: 89, completionTokens: 1_500_000 }),
          makeListRow(3, { promptTokens: 12, completionTokens: 34 }),
          // 无 usage：显示占位符
          makeListRow(4),
        ],
        4,
      ),
    );
    renderViewer();

    await screen.findAllByText("/v1/messages");
    // Anthropic：↑1.2k ↓567（千分位紧凑）
    expect(screen.getByText("↑1.2k")).toBeDefined();
    expect(screen.getByText("↓567")).toBeDefined();
    // OpenAI：↑89 ↓1.5M
    expect(screen.getByText("↑89")).toBeDefined();
    expect(screen.getByText("↓1.5M")).toBeDefined();
    // Gemini：↑12 ↓34
    expect(screen.getByText("↑12")).toBeDefined();
    expect(screen.getByText("↓34")).toBeDefined();
    // 无 usage 的行 + 表头处也各有一个占位 —
    expect(screen.getAllByText("—").length).toBeGreaterThanOrEqual(1);
  });

  it("copies an executable cURL command from the request detail", async () => {
    getRecords.mockResolvedValue(page([makeListRow(1)], 1));
    renderViewer();

    await expandFirstRecord();
    fireEvent.click(screen.getByText("proxy.requestLogViewer.copyAsCurl"));

    await waitFor(() => expect(copyTextMock).toHaveBeenCalled());
    const command = copyTextMock.mock.calls[0][0] as string;
    expect(command).toContain(
      "curl -X POST 'https://api.anthropic.com/v1/messages'",
    );
    // 有效头保留
    expect(command).toContain("-H 'content-type: application/json'");
    expect(command).toContain("-H 'authorization: Bearer sk-test'");
    // hop-by-hop / 长度类头剔除（重放时由 curl 重新生成）
    expect(command).not.toContain("-H 'host:");
    expect(command).not.toContain("-H 'content-length:");
    expect(command).toContain("-d '");
  });

  it("uses the recorded outbound URL in the cURL command", async () => {
    getRecords.mockResolvedValue(page([makeListRow(1)], 1));
    getRecord.mockImplementation(async () =>
      makeRecord(1, {
        endpoint: "/v1/responses",
        url: "https://gateway.example.com/base/v1/responses?api-version=2026-09-01",
      }),
    );
    renderViewer();

    await expandFirstRecord();
    fireEvent.click(screen.getByText("proxy.requestLogViewer.copyAsCurl"));

    await waitFor(() => expect(copyTextMock).toHaveBeenCalled());
    const command = copyTextMock.mock.calls[0][0] as string;
    expect(command).toContain(
      "curl -X POST 'https://gateway.example.com/base/v1/responses?api-version=2026-09-01'",
    );
  });

  it("omits -X for GET requests in the cURL command", async () => {
    getRecords.mockResolvedValue(page([makeListRow(1)], 1));
    getRecord.mockImplementation(async () =>
      makeRecord(1, {
        method: "GET",
        endpoint: "/v1/models",
        url: "https://api.anthropic.com/v1/models",
        requestBody: undefined,
      }),
    );
    renderViewer();

    await expandFirstRecord();
    fireEvent.click(screen.getByText("proxy.requestLogViewer.copyAsCurl"));

    await waitFor(() => expect(copyTextMock).toHaveBeenCalled());
    const command = copyTextMock.mock.calls[0][0] as string;
    expect(command).toContain("curl 'https://api.anthropic.com/v1/models'");
    expect(command).not.toContain("-X");
    expect(command).not.toContain("-d ");
  });

  it("reloads with the debounced search keyword", async () => {
    renderViewer();
    await screen.findAllByText("/v1/messages");
    getRecords.mockClear();

    fireEvent.change(
      screen.getByPlaceholderText("proxy.requestLogViewer.searchPlaceholder"),
      { target: { value: "claude" } },
    );

    await waitFor(
      () => {
        expect(getRecords).toHaveBeenCalledWith(
          expect.objectContaining({ search: "claude" }),
        );
      },
      { timeout: 2000 },
    );
  });

  it("falls back to the first remaining file when the selected one disappears", async () => {
    renderViewer();
    await screen.findAllByText("/v1/messages");
    expect(getRecords).toHaveBeenCalledWith(
      expect.objectContaining({ fileName: "session-a.jsonl" }),
    );

    // 刷新后 session-a 被清理：选中应回退到剩余的第一个文件
    listFiles.mockResolvedValue([files[1]]);
    getRecords.mockClear();
    // 刷新按钮在左栏 Card 头部，是仅图标按钮（title 提示）
    fireEvent.click(screen.getByTitle("proxy.requestLogViewer.refresh"));

    await waitFor(() => {
      expect(getRecords).toHaveBeenCalledWith(
        expect.objectContaining({ fileName: "session-b.jsonl" }),
      );
    });
  });

  it("ignores stale detail responses after switching sessions", async () => {
    let resolveFirstDetail: (record: Record<string, unknown>) => void = () => {};
    getRecords.mockImplementation(async (params: { fileName: string }) => {
      if (params.fileName === "session-a.jsonl") {
        return page([makeListRow(1)], 1);
      }
      return page([makeListRow(1)], 1);
    });
    getRecord
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveFirstDetail = resolve;
          }),
      )
      .mockImplementationOnce(async () =>
        makeRecord(1, {
          endpoint: "/session-b",
          url: "https://api.example.com/session-b",
          requestBody: { session: "b" },
        }),
      );

    renderViewer();
    await screen.findByText("Session A");
    fireEvent.click(screen.getAllByRole("button", { expanded: false })[0]);
    await screen.findByText("proxy.requestLogViewer.loading");

    fireEvent.click(screen.getByText("Session B"));
    await waitFor(() =>
      expect(getRecords).toHaveBeenCalledWith(
        expect.objectContaining({ fileName: "session-b.jsonl" }),
      ),
    );
    fireEvent.click(screen.getAllByRole("button", { expanded: false })[0]);

    resolveFirstDetail(
      makeRecord(1, {
        endpoint: "/session-a-stale",
        requestBody: { session: "a" },
      }),
    );

    await screen.findByText("proxy.requestLogViewer.copyAsCurl");
    fireEvent.click(screen.getByText("proxy.requestLogViewer.copyAsCurl"));
    await waitFor(() => expect(copyTextMock).toHaveBeenCalled());
    const command = copyTextMock.mock.calls[0][0] as string;
    expect(command).toContain("https://api.example.com/session-b");
    expect(command).not.toContain("/session-a-stale");
  });

  it("renders the full body without line truncation", async () => {
    // 大 body 必须完整渲染：不允许展示层截断
    const bigBody = {
      items: Array.from({ length: 250 }, (_, i) => ({
        index: i,
        text: `row-${i}`,
      })),
    };
    getRecords.mockResolvedValue(page([makeListRow(1)], 1));
    getRecord.mockImplementation(async () =>
      makeRecord(1, { requestBody: bigBody }),
    );
    renderViewer();

    await expandFirstRecord();
    const content = document.querySelector(".cm-content");
    expect(content).not.toBeNull();
    const view = EditorView.findFromDOM(content as HTMLElement);
    expect(view).toBeDefined();
    // 250 个对象 × 4 行（{ index text }）+ 首尾 + items 属性行 = 1004 行，完整渲染
    expect(view!.state.doc.lines).toBe(1004);
  });

  it("folds to level 1 including the first property while scalars stay expanded", async () => {
    getRecords.mockResolvedValue(page([makeListRow(1)], 1));
    getRecord.mockImplementation(async () =>
      makeRecord(1, {
        requestBody: {
          items: Array.from({ length: 20 }, (_, i) => i),
          model: "x",
        },
      }),
    );
    renderViewer();

    await expandFirstRecord();
    fireEvent.click(screen.getByText("proxy.requestLogViewer.foldToLevel1"));

    const content = document.querySelector(".cm-content");
    const view = EditorView.findFromDOM(content as HTMLElement)!;
    const folded: Array<{ from: number; to: number }> = [];
    view.state
      .field(foldState)
      .between(0, view.state.doc.length, (from, to) => {
        folded.push({ from, to });
      });

    // 一级属性全部折叠（items 是唯一的复合值属性），标量属性 model 不折
    expect(folded).toHaveLength(1);
    // 折叠起点落在第 2 行（第一个属性）——从 i=2 开始遍历会漏掉它
    const secondLine = view.state.doc.line(2);
    expect(folded[0].from).toBeGreaterThanOrEqual(secondLine.from);
    expect(folded[0].from).toBeLessThanOrEqual(secondLine.to);

    // 同一按钮切换为「全部展开」；点击后折叠区间清空、按钮回到「折叠到一级」
    expect(screen.getByText("proxy.requestLogViewer.unfoldAll")).toBeDefined();
    fireEvent.click(screen.getByText("proxy.requestLogViewer.unfoldAll"));
    const remaining: number[] = [];
    view.state.field(foldState).between(0, view.state.doc.length, () => {
      remaining.push(1);
    });
    expect(remaining).toHaveLength(0);
    expect(
      screen.getByText("proxy.requestLogViewer.foldToLevel1"),
    ).toBeDefined();
  });

  it("only marks structure-opening lines as foldable in the gutter", async () => {
    // gutter 折叠按钮应只出现在开括号行/复合值属性行，不出现在值内部的行
    getRecords.mockResolvedValue(page([makeListRow(1)], 1));
    getRecord.mockImplementation(async () =>
      makeRecord(1, {
        requestBody: {
          messages: [
            { role: "user", content: "hi" },
            { role: "assistant", content: "hello" },
          ],
          model: "x",
        },
      }),
    );
    renderViewer();

    await expandFirstRecord();
    const view = EditorView.findFromDOM(
      document.querySelector(".cm-content") as HTMLElement,
    )!;
    const { foldable } = await import("@codemirror/language");

    // 逐行求 foldable()：foldGutter 的 gutter 标记就是按行调它生成的
    const doc = view.state.doc;
    const foldableLines: number[] = [];
    for (let n = 1; n <= doc.lines; n++) {
      const line = doc.line(n);
      if (foldable(view.state, line.from, line.to)) foldableLines.push(n);
    }

    // 布局：
    //  1  {                     ← Object 开括号，foldInside
    //  2    "messages": [       ← 复合值属性行，valueFoldService
    //  3      {                 ← Object 开括号
    //  4/5     role/content     ← 值内部行：不得有按钮（修复前这里会命中外层属性）
    //  6      },
    //  7      {                 ← 第二个元素
    //  8/9     role/content
    // 10      },
    // 11    ],
    // 12    "model": "x"        ← 标量属性行：无按钮
    // 13  }
    expect(foldableLines).toEqual([1, 2, 3, 7]);
  });

  it("prepends new records on top-scroll and dedupes overlapping line numbers", async () => {
    getRecords.mockResolvedValueOnce(page([makeListRow(5), makeListRow(4)], 2));
    renderViewer();
    await screen.findAllByText("/v1/messages");
    expect(getRecords).toHaveBeenCalledWith(
      expect.objectContaining({ order: "desc" }),
    );

    // 顶部滚动（desc 模式）→ 拉取 lineNo > 5 的新记录；返回值与已有记录重叠（5）
    getRecords.mockResolvedValueOnce(
      page([makeListRow(7), makeListRow(6), makeListRow(5)], 7),
    );
    fireEvent.scroll(getScrollContainer());

    await waitFor(() => {
      expect(getRecords).toHaveBeenCalledWith(
        expect.objectContaining({ afterLineNo: 5, order: "desc" }),
      );
    });
    // 去重后 4 条：7、6、5、4（5 不重复出现）
    await waitFor(() => {
      expect(screen.getAllByText("/v1/messages")).toHaveLength(4);
    });
  });
});
