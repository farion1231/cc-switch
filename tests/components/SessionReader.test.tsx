import type { ReactElement } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { buildInputDiff } from "@/components/sessions/reader/SessionDiff";
import { SessionReader } from "@/components/sessions/reader/SessionReader";
import { sessionsApi } from "@/lib/api/sessions";
import {
  clearSessionImageCache,
  type SessionTranscriptResult,
} from "@/lib/query/sessions";
import type { SessionMessage, SessionMeta } from "@/types";
import { call, fixtures, msg, result, text } from "./sessions/reader/helpers";
import { createTestQueryClient } from "../utils/testQueryClient";

// jsdom 没有布局，虚拟列表一条都不渲染；这里让它把全部行都画出来
vi.mock("@tanstack/react-virtual", () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    getTotalSize: () => count * 40,
    getVirtualItems: () =>
      Array.from({ length: count }, (_, index) => ({
        index,
        key: index,
        start: index * 40,
      })),
    measureElement: () => undefined,
    scrollToIndex: () => undefined,
  }),
}));

const PROJECT = "/Users/yovinchen/Projects/demo-app";

const meta = (providerId: string): SessionMeta => ({
  providerId,
  sessionId: `${providerId}-session`,
  title: `${providerId} 会话`,
  projectDir: PROJECT,
  createdAt: 1791014400000,
  lastActiveAt: 1791018000000,
  sourcePath: `/mock/${providerId}.jsonl`,
  resumeCommand: `${providerId} resume`,
});

const transcriptOf = (messages: SessionMessage[]): SessionTranscriptResult => ({
  header: null,
  messages,
  isStreaming: false,
  progress: { loaded: messages.length, total: messages.length },
  isLoading: false,
  isError: false,
  error: null,
  refetch: async () => undefined,
});

const onCopy = vi.fn();

const renderReader = (
  providerId: string,
  messages: SessionMessage[],
): ReturnType<typeof render> => {
  const client = createTestQueryClient();
  const ui: ReactElement = (
    <QueryClientProvider client={client}>
      <SessionReader
        session={meta(providerId)}
        appName={providerId === "claude" ? "Claude Code" : providerId}
        transcript={transcriptOf(messages)}
        listQuery=""
        launchTerminal={null}
        hasPrev={false}
        hasNext={false}
        onPrev={vi.fn()}
        onNext={vi.fn()}
        onBack={vi.fn()}
        onLaunch={vi.fn()}
        onCopy={onCopy}
        onOpenTerminalSettings={vi.fn()}
        onReload={vi.fn()}
        onDelete={vi.fn()}
      />
    </QueryClientProvider>
  );
  return render(ui);
};

/** 正文滚动区（右侧对话目录里也有同样的摘要，断言正文时限定在这里） */
const conversation = () => screen.getByRole("region", { name: "对话内容" });

/** 步骤行按钮（accessible name 带完整标题与状态） */
const stepButton = (name: RegExp) => screen.getByRole("button", { name });

describe("SessionReader", () => {
  beforeEach(() => {
    onCopy.mockReset();
    window.localStorage.clear();
    clearSessionImageCache();
    // jsdom 没有 IntersectionObserver，图片会直接加载；统一给个假图
    URL.createObjectURL = vi.fn(() => "blob:mock-image");
    URL.revokeObjectURL = vi.fn();
    vi.spyOn(sessionsApi, "getImage").mockResolvedValue(new ArrayBuffer(8));
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe("五家 Agent 共用骨架，只换符号、动词与合并规则", () => {
    it("Claude：⏺ 符号、Name(arg) 标题、失败步骤常显、主题色", () => {
      const { container } = renderReader("claude", fixtures.claude);

      expect(
        container.querySelector('[data-reader-style="claude"]'),
      ).toHaveStyle({ "--reader-accent": "var(--agent-claude)" });
      // 标题用会话标题，不是路径
      expect(
        screen.getByRole("heading", { level: 1, name: "claude 会话" }),
      ).toBeInTheDocument();
      expect(
        within(conversation()).getByText(
          /cargo build 一直报 tokio_util 找不到/,
        ),
      ).toBeInTheDocument();
      // 第一轮有最终回复 → 执行过程折叠；失败的 Bash 仍常显并带 exit
      expect(
        screen.getAllByRole("button", { name: /^展开执行过程/ }).length,
      ).toBeGreaterThan(0);
      const failed = stepButton(/Bash\(cargo build 2>&1 \| tail -50\)/);
      expect(failed).toHaveAttribute("aria-expanded", "true");
      expect(within(failed).getByText("exit 101")).toBeInTheDocument();
      // 成功的 Edit 折在摘要里，Claude 显示为 Update
      expect(
        screen.queryByRole("button", { name: /Update\(Cargo\.toml\)/ }),
      ).not.toBeInTheDocument();
      // 最终回复：⏺ + Agent 名 + 模型
      expect(screen.getAllByText("Claude Code").length).toBeGreaterThan(0);
      expect(screen.getAllByText(/claude-opus-5-5/).length).toBeGreaterThan(0);
      // 事件：PR 链接、上下文压缩、Hook 错误
      expect(
        screen.getByRole("button", { name: /在浏览器中打开/ }),
      ).toHaveTextContent("已关联 PR #128");
      expect(screen.getByText("上下文已压缩")).toBeInTheDocument();
      expect(screen.getByText("Hook 执行出错")).toBeInTheDocument();
    });

    it("Claude：连续成功的同一 MCP 服务器调用合并为 Called X N times", () => {
      const mcp = (id: string) =>
        call(id, "mcp", {
          rawName: "mcp__slack__post",
          title: "slack.post",
          server: "slack",
        });
      renderReader("claude", [
        msg("user", [text("发两条消息")], { turnId: "t1" }),
        msg("assistant", [mcp("m1"), mcp("m2")], { turnId: "t1" }),
        msg("tool", [result("m1"), result("m2")], { turnId: "t1" }),
        msg("assistant", [call("b1")], { turnId: "t1" }),
        msg("tool", [result("b1", "error")], { turnId: "t1" }),
      ]);
      // 没有最终回复 → 整轮自动展开
      expect(stepButton(/Called slack 2 times/)).toBeInTheDocument();
    });

    it("Codex：• 符号、Ran / Search 动词、连续读取合并为 Explored", () => {
      renderReader("codex", fixtures.codex);
      expect(stepButton(/Search rg -n "--legacy" docs/)).toHaveAttribute(
        "aria-expanded",
        "true",
      );
      // 第二轮被中断 → 自动展开，能看到中断的命令
      expect(stepButton(/Ran pnpm build/)).toBeInTheDocument();
      expect(screen.getAllByText("已中断").length).toBeGreaterThan(0);
      // 注入的 AGENTS.md 默认隐藏
      expect(screen.queryByText(/上下文 · AGENTS\.md/)).not.toBeInTheDocument();
    });

    it("Codex：连续成功的 read / search 合并成 Explored", () => {
      renderReader("codex", [
        msg("user", [text("看看代码")], { turnId: "t1" }),
        msg(
          "assistant",
          [
            call("r1", "read", { title: "a.ts" }),
            call("s1", "search", { title: "foo in src" }),
          ],
          { turnId: "t1" },
        ),
        msg("tool", [result("r1"), result("s1")], { turnId: "t1" }),
        msg("assistant", [call("x1")], { turnId: "t1" }),
        msg("tool", [result("x1", "error")], { turnId: "t1" }),
      ]);
      const explored = stepButton(/Explored/);
      expect(explored).toHaveTextContent("查看了 1 个文件、搜索 1 次");
      fireEvent.click(explored);
      expect(stepButton(/Read a\.ts/)).toBeInTheDocument();
    });

    it("Gemini：✓ / x 状态符与 ✦ 助手符号", () => {
      renderReader("gemini", fixtures.gemini);
      // 第一轮失败的 build 用 x，第二轮被中断的用 -
      const [failed, interrupted] = screen.getAllByRole("button", {
        name: /Shell npm run build/,
      });
      expect(within(failed).getByText("x")).toBeInTheDocument();
      expect(within(interrupted).getByText("-")).toBeInTheDocument();
      expect(screen.getAllByText("✦").length).toBeGreaterThan(0);
      expect(
        screen.getByText(/You have exhausted your capacity/),
      ).toBeInTheDocument();
    });

    it("OpenCode：$ / → 图标标题，摘要合计 tokens 与费用", () => {
      renderReader("opencode", fixtures.opencode);
      expect(stepButton(/\$ bunx biome --version/)).toHaveAttribute(
        "aria-expanded",
        "true",
      );
      const summaries = screen.getAllByRole("button", {
        name: /执行过程/,
      });
      expect(summaries[0].textContent).toMatch(/tok/);
      expect(summaries[0].textContent).toMatch(/\$0\.0\d+/);
    });

    it("Pi：小写动词与 ● 状态符", () => {
      renderReader("pi", fixtures.pi);
      const failed = stepButton(/bash pnpm test -- LoginButton（失败）/);
      expect(within(failed).getByText("●")).toBeInTheDocument();
    });

    it("其余来源用通用动词（i18n）", () => {
      renderReader("hermes", fixtures.generic);
      fireEvent.click(
        screen.getAllByRole("button", { name: /^展开执行过程/ })[0],
      );
      expect(stepButton(/运行 df -h \//)).toBeInTheDocument();
    });
  });

  it("失败步骤常显，输出预览默认展开前 6 行", () => {
    renderReader("claude", fixtures.claude);
    const region = screen.getByRole("region", {
      name: /Bash\(cargo build 2>&1 \| tail -50\)/,
    });
    expect(region).toHaveTextContent("error[E0433]");
    expect(region).toHaveTextContent("还有 82 行");
  });

  it("参数超过预览上限的 shell 命令：显示「参数」区并按需取完整命令", async () => {
    const longCommand = `echo ${"x".repeat(560)} && echo LONG_COMMAND_TAIL`;
    const fullInput = JSON.stringify({ command: longCommand });
    const getBlockContent = vi
      .spyOn(sessionsApi, "getBlockContent")
      .mockResolvedValue({
        text: fullInput,
        totalLen: fullInput.length,
        truncated: false,
      });
    const messages: SessionMessage[] = [
      {
        role: "user",
        content: "run it",
        ts: 1791014400000,
        turnId: "t1",
        blocks: [{ type: "text", text: "run it" }],
      },
      {
        role: "assistant",
        content: "",
        ts: 1791014401000,
        turnId: "t1",
        blocks: [
          {
            type: "tool_call",
            id: "toolu_long",
            rawName: "Bash",
            kind: "shell",
            title: longCommand.slice(0, 200),
            inputPreview: fullInput.slice(0, 400),
            inputTotalLen: fullInput.length,
            inputFull: {
              kind: "jsonl",
              offset: 10,
              len: 700,
              pointer: "/message/content/0/input",
            },
          },
        ],
      },
      {
        role: "tool",
        content: "",
        ts: 1791014402000,
        turnId: "t1",
        blocks: [
          {
            type: "tool_result",
            callId: "toolu_long",
            status: "success",
            preview: "ok",
            totalLen: 2,
            lineCount: 1,
            truncated: false,
          },
        ],
      },
      {
        role: "assistant",
        content: "",
        ts: 1791014403000,
        turnId: "t1",
        blocks: [{ type: "text", text: "done" }],
      },
    ];
    renderReader("claude", messages);

    fireEvent.click(stepButton(/Bash\(echo x/));
    const region = screen.getByRole("region", { name: /Bash\(echo x/ });
    expect(within(region).getByText("参数")).toBeInTheDocument();
    expect(region).not.toHaveTextContent("LONG_COMMAND_TAIL");

    fireEvent.click(within(region).getByRole("button", { name: "显示全部" }));
    expect(
      await within(region).findByText(/LONG_COMMAND_TAIL/),
    ).toBeInTheDocument();
    expect(getBlockContent).toHaveBeenCalledWith(
      "claude",
      "/mock/claude.jsonl",
      expect.objectContaining({ kind: "jsonl", offset: 10 }),
      expect.objectContaining({ offset: 0 }),
    );
  });

  it("「显示全部」按需取全文", async () => {
    const getBlockContent = vi
      .spyOn(sessionsApi, "getBlockContent")
      .mockResolvedValue({
        text: "FULL OUTPUT line 1\nFULL OUTPUT line 88",
        totalLen: 40,
        truncated: false,
      });
    renderReader("claude", fixtures.claude);
    const region = screen.getByRole("region", {
      name: /Bash\(cargo build 2>&1 \| tail -50\)/,
    });
    expect(getBlockContent).not.toHaveBeenCalled();
    fireEvent.click(within(region).getByRole("button", { name: "显示全部" }));

    expect(
      await within(region).findByText(/FULL OUTPUT line 88/),
    ).toBeInTheDocument();
    expect(getBlockContent).toHaveBeenCalledWith(
      "claude",
      "/mock/claude.jsonl",
      expect.objectContaining({ kind: "jsonl", offset: 201733 }),
      expect.objectContaining({ offset: 0 }),
    );
  });

  it("压缩摘要只下发预览，展开后按引用取全文", async () => {
    const getBlockContent = vi
      .spyOn(sessionsApi, "getBlockContent")
      .mockResolvedValue({
        text: "Summary: FULL COMPACTION SUMMARY",
        totalLen: 31,
        truncated: false,
      });
    renderReader("codex", fixtures.codex);
    fireEvent.click(screen.getByRole("button", { name: /上下文已压缩/ }));
    expect(getBlockContent).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "显示全部" }));

    expect(
      await screen.findByText(/FULL COMPACTION SUMMARY/),
    ).toBeInTheDocument();
    expect(getBlockContent).toHaveBeenCalledWith(
      "codex",
      "/mock/codex.jsonl",
      expect.objectContaining({ kind: "jsonl", pointer: "/payload/message" }),
      expect.objectContaining({ offset: 0 }),
    );
  });

  it("图片进入视口才请求，缩略图可放大", async () => {
    const observers: IntersectionObserverCallback[] = [];
    vi.stubGlobal(
      "IntersectionObserver",
      class {
        constructor(callback: IntersectionObserverCallback) {
          observers.push(callback);
        }
        observe() {}
        disconnect() {}
        unobserve() {}
      },
    );
    const getImage = vi.mocked(sessionsApi.getImage);

    renderReader("claude", fixtures.claude);
    expect(screen.getByRole("img", { name: "图 1" })).toHaveAttribute(
      "aria-busy",
      "true",
    );
    expect(getImage).not.toHaveBeenCalled();

    act(() => {
      observers.forEach((callback) =>
        callback(
          [{ isIntersecting: true } as IntersectionObserverEntry],
          {} as IntersectionObserver,
        ),
      );
    });
    await waitFor(() => expect(getImage).toHaveBeenCalled());
    const open = await screen.findByRole("button", { name: /放大查看：图 1/ });
    expect(within(open).getByRole("img")).toHaveAttribute(
      "src",
      "blob:mock-image",
    );
    fireEvent.click(open);
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    vi.unstubAllGlobals();
  });

  it("查找命中在折叠的步骤里时自动展开该轮与该步", async () => {
    renderReader("claude", fixtures.claude);
    expect(
      screen.queryByRole("button", { name: /Grep\(tokio_util in src\)/ }),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "在会话中查找" }));
    fireEvent.change(screen.getByRole("textbox", { name: "查找内容" }), {
      target: { value: "LinesCodec;" },
    });
    await waitFor(() =>
      expect(stepButton(/Grep\(tokio_util in src\)/)).toHaveAttribute(
        "aria-expanded",
        "true",
      ),
    );
    expect(
      screen.getByRole("region", { name: /Grep\(tokio_util in src\)/ }),
    ).toHaveTextContent("src/net/client.rs:7");
    expect(screen.getByText(/^1 \/ \d+$/)).toBeInTheDocument();
  });

  it("Unicode 查找数量、高亮与上下处导航保持一致", async () => {
    renderReader("claude", [
      msg("user", [text("İstanbul")], { turnId: "t1" }),
      msg("assistant", [text("İZMİR")], { turnId: "t1" }),
    ]);
    fireEvent.click(screen.getByRole("button", { name: "在会话中查找" }));
    fireEvent.change(screen.getByRole("textbox", { name: "查找内容" }), {
      target: { value: "i\u0307" },
    });

    expect(await screen.findByText("1 / 3")).toBeInTheDocument();
    expect(
      Array.from(
        conversation().querySelectorAll("mark"),
        (mark) => mark.textContent,
      ),
    ).toEqual(["İ", "İ", "İ"]);
    fireEvent.click(screen.getByRole("button", { name: "下一个匹配" }));
    expect(await screen.findByText("2 / 3")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "上一个匹配" }));
    expect(await screen.findByText("1 / 3")).toBeInTheDocument();
  });

  it.each([
    ["İ".repeat(100) + "/NEEDLE/" + "x".repeat(200), "needle", "NEEDLE"],
    ["x".repeat(80) + "/İ/" + "x".repeat(80), "i\u0307", "İ"],
    ["A" + "\u0301".repeat(100) + "Σ", "ς", "Σ"],
    [
      "😀" + "a".repeat(59) + "NEEDLE" + "b".repeat(59) + "😀",
      "needle",
      "NEEDLE",
    ],
  ])(
    "原文片段把 Unicode 小写偏移映射回原文：%s",
    async (path, query, expected) => {
      renderReader("claude", [
        msg("user", [text("查看链接")], { turnId: "t1" }),
        msg("assistant", [text(`[docs](https://example.test/${path})`)], {
          turnId: "t1",
        }),
      ]);
      fireEvent.click(screen.getByRole("button", { name: "在会话中查找" }));
      fireEvent.change(screen.getByRole("textbox", { name: "查找内容" }), {
        target: { value: query },
      });

      expect(await screen.findByText("1 / 1")).toBeInTheDocument();
      const rawMatch =
        within(conversation()).getByText("原文中的匹配").parentElement!;
      expect(
        Array.from(
          rawMatch.querySelectorAll("mark"),
          (mark) => mark.textContent,
        ),
      ).toEqual([expected]);
      if (path.includes("😀")) {
        expect(rawMatch.textContent?.match(/😀/gu)).toHaveLength(2);
      }
      expect(
        within(conversation()).getByRole("link", { name: "docs" }),
      ).toBeInTheDocument();
    },
  );

  it("保留相邻原文片段重叠区域里的全部高亮", async () => {
    const path = "x" + "a".repeat(49) + "x" + "a".repeat(49) + "x";
    renderReader("claude", [
      msg("user", [text("查看链接")], { turnId: "t1" }),
      msg("assistant", [text(`[docs](https://host.test/${path})`)], {
        turnId: "t1",
      }),
    ]);
    fireEvent.click(screen.getByRole("button", { name: "在会话中查找" }));
    fireEvent.change(screen.getByRole("textbox", { name: "查找内容" }), {
      target: { value: "x" },
    });

    expect(await screen.findByText("1 / 3")).toBeInTheDocument();
    const rawMatch =
      within(conversation()).getByText("原文中的匹配").parentElement!;
    const snippets = rawMatch.querySelectorAll("p");
    expect(snippets).toHaveLength(2);
    for (const snippet of snippets) {
      expect(
        Array.from(
          snippet.querySelectorAll("mark"),
          (mark) => mark.textContent,
        ),
      ).toEqual(["x", "x"]);
    }
  });

  it("右侧对话目录区分人和 Agent，可开关", () => {
    renderReader("claude", fixtures.claude);
    const outline = screen.getByRole("navigation", { name: "对话目录" });
    // 每轮一条「你」的提问 + 一条 Agent 的输出
    const questions = within(outline).getAllByText("你", { exact: false });
    expect(questions.length).toBeGreaterThan(0);
    expect(
      within(outline).getByText(/cargo build 一直报 tokio_util 找不到/),
    ).toBeInTheDocument();
    expect(within(outline).getByText(/构建已经通过/)).toBeInTheDocument();

    // 点 Agent 那条能跳过去（不报错即可，滚动由虚拟列表负责）
    fireEvent.click(within(outline).getByText(/构建已经通过/));

    // 关掉后目录消失，开关记住状态
    fireEvent.click(screen.getByRole("button", { name: "对话目录" }));
    expect(
      screen.queryByRole("navigation", { name: "对话目录" }),
    ).not.toBeInTheDocument();
    expect(window.localStorage.getItem("cc-switch.sessionReader.outline")).toBe(
      "false",
    );
  });

  it("章节：正文里成行、目录里可点、导出为小标题", async () => {
    const chapter = (id: string, title: string, detail?: string) =>
      call(id, "mcp", {
        rawName: "mcp__ccd_session__mark_chapter",
        server: "ccd_session",
        title,
        detail,
      });
    renderReader("claude", [
      msg("user", [text("重构支付模块")], { turnId: "t1" }),
      msg(
        "assistant",
        [chapter("ch1", "梳理现状", "先读代码"), call("c1"), call("c2")],
        { turnId: "t1" },
      ),
      msg("tool", [result("ch1"), result("c1"), result("c2")], {
        turnId: "t1",
      }),
      msg("assistant", [text("改完了")], { turnId: "t1" }),
    ]);

    // 正文：章节单独成行，不计入执行步骤，也不显示成 MCP 调用
    expect(within(conversation()).getByText("梳理现状")).toBeInTheDocument();
    expect(within(conversation()).getByText("先读代码")).toBeInTheDocument();
    expect(screen.getByText(/执行过程 · 2 步/)).toBeInTheDocument();
    expect(screen.queryByText(/mark_chapter/)).not.toBeInTheDocument();

    // 目录：章节标题可点（不报错即可，滚动由虚拟列表负责）
    const outline = screen.getByRole("navigation", { name: "对话目录" });
    fireEvent.click(within(outline).getByRole("button", { name: /梳理现状/ }));

    await userEvent.click(
      screen.getByRole("button", { name: "claude 会话 的更多操作" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "复制整段为 Markdown" }),
    );
    const [markdown] = onCopy.mock.calls[0] as [string, string];
    expect(markdown).toContain("### 梳理现状\n\n> 先读代码");
  });

  it("导出为 Markdown 文件：用会话标题做文件名，内容与复制为 Markdown 一致", async () => {
    const exportMarkdown = vi
      .spyOn(sessionsApi, "exportMarkdown")
      .mockResolvedValue("/tmp/claude 会话.md");
    renderReader("claude", fixtures.claude);

    fireEvent.click(
      screen.getByRole("button", { name: "导出为 Markdown 文件" }),
    );

    await waitFor(() => expect(exportMarkdown).toHaveBeenCalledTimes(1));
    const [name, content] = exportMarkdown.mock.calls[0];
    expect(name).toBe("claude 会话.md");
    expect(content).toContain("claude 会话");
    expect(content).toContain("cargo build 一直报 tokio_util 找不到");
  });

  it("全部 / 对话 / 改动 三选一", () => {
    renderReader("claude", fixtures.claude);
    fireEvent.click(screen.getByRole("button", { name: "对话" }));
    expect(screen.getByRole("button", { name: "对话" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(
      screen.queryByRole("button", { name: /Bash\(/ }),
    ).not.toBeInTheDocument();
    expect(
      within(conversation()).getByText(/cargo build 一直报 tokio_util 找不到/),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "改动" }));
    expect(screen.getByRole("button", { name: "对话" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    expect(stepButton(/Update\(Cargo\.toml\)/)).toBeInTheDocument();
    expect(stepButton(/Write\(CHANGELOG\.md\)/)).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Bash\(/ }),
    ).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "全部" }));
    expect(screen.getByRole("button", { name: "全部" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("复制 Markdown 含步骤列表，默认不含思考", async () => {
    renderReader("claude", fixtures.claude);
    await userEvent.click(
      screen.getByRole("button", { name: "claude 会话 的更多操作" }),
    );
    await userEvent.click(
      await screen.findByRole("menuitem", { name: "复制整段为 Markdown" }),
    );
    expect(onCopy).toHaveBeenCalledTimes(1);
    const [markdown] = onCopy.mock.calls[0] as [string, string];
    expect(markdown).toMatch(/^# claude 会话/);
    expect(markdown).toContain("**你**");
    expect(markdown).toContain(
      "- ⏺ Bash(cargo build 2>&1 | tail -50) — 失败 · exit 101 · 12s",
    );
    expect(markdown).toContain("- ⏺ Update(Cargo.toml) — +1 −0");
    expect(markdown).toContain("**Claude Code**");
    expect(markdown).not.toContain("先看 Cargo.toml 里的 [dependencies]");
  });

  it("复制这一轮", () => {
    renderReader("claude", fixtures.claude);
    fireEvent.click(screen.getAllByRole("button", { name: "复制这一轮" })[0]);
    const [markdown, message] = onCopy.mock.calls[0] as [string, string];
    expect(message).toBe("已复制这一轮");
    expect(markdown).toContain("Grep(tokio_util in src)");
  });

  it("键盘：Enter 展开折叠的执行过程与步骤", async () => {
    const user = userEvent.setup();
    renderReader("claude", fixtures.claude);
    const summary = screen.getAllByRole("button", {
      name: /^展开执行过程/,
    })[0];
    summary.focus();
    await user.keyboard("{Enter}");
    expect(summary).toHaveAttribute("aria-expanded", "true");

    const read = stepButton(/Read\(Cargo\.toml:1-40\)/);
    expect(read).toHaveAttribute("aria-expanded", "false");
    read.focus();
    await user.keyboard(" ");
    expect(stepButton(/Read\(Cargo\.toml:1-40\)/)).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(
      screen.getByRole("region", { name: /Read\(Cargo\.toml:1-40\)/ }),
    ).toHaveTextContent("tokio = ");
  });

  it("显示注入的上下文（默认隐藏）", async () => {
    renderReader("codex", fixtures.codex);
    expect(screen.queryByText(/上下文 · /)).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "显示选项" }));
    await userEvent.click(
      await screen.findByRole("menuitemcheckbox", { name: "显示注入的上下文" }),
    );
    expect((await screen.findAllByText(/上下文 · /)).length).toBeGreaterThan(0);
  });

  it("流式加载中先显示已到的轮次与进度", () => {
    const client = new QueryClient();
    const partial = fixtures.claude.slice(0, 4);
    render(
      <QueryClientProvider client={client}>
        <SessionReader
          session={meta("claude")}
          appName="Claude Code"
          transcript={{
            ...transcriptOf(partial),
            isStreaming: true,
            progress: { loaded: 4, total: fixtures.claude.length },
          }}
          listQuery=""
          launchTerminal={null}
          hasPrev={false}
          hasNext={false}
          onPrev={vi.fn()}
          onNext={vi.fn()}
          onBack={vi.fn()}
          onLaunch={vi.fn()}
          onCopy={onCopy}
          onOpenTerminalSettings={vi.fn()}
          onReload={vi.fn()}
          onDelete={vi.fn()}
        />
      </QueryClientProvider>,
    );
    expect(
      screen.getByText(`已加载 4 / ${fixtures.claude.length}`),
    ).toBeInTheDocument();
    expect(
      within(conversation()).getByText(/cargo build 一直报 tokio_util 找不到/),
    ).toBeInTheDocument();
  });

  it("Antigravity 工具详情：还原大驼峰改动、隐藏空改动标题、展示多行 CommandLine 与无命令 shell 参数", () => {
    const editInput = JSON.stringify({
      TargetFile: `${PROJECT}/src/main.rs`,
      ReplacementChunks: [
        {
          TargetContent: "old_line",
          ReplacementContent: "new_line_1\nnew_line_2",
        },
      ],
    });
    const truncatedWriteInput = JSON.stringify({
      Path: `${PROJECT}/src/big.rs`,
      Content: "fn main() {}\n".repeat(40),
    });
    const multiCmdInput = JSON.stringify({
      CommandLine: "export FOO=1\ncargo test --lib agy",
      WaitMsBeforeAsync: 5000,
    });
    const taskInput = JSON.stringify({
      Action: "status",
      TaskId: "conv-1/task-8",
    });

    const fullInput = (obj: Record<string, unknown>) => {
      const s = JSON.stringify(obj);
      return { inputPreview: s, inputTotalLen: s.length };
    };

    expect(
      buildInputDiff(
        call("c-edit", "edit", {
          rawName: "edit_file",
          ...fullInput({
            Path: "/a.rs",
            Target: "before",
            Replacement: "after",
          }),
        }),
      ),
    ).toBe("@@\n-before\n+after");
    expect(
      buildInputDiff(
        call("c-incomplete", "edit", {
          rawName: "multi_replace_file_content",
          ...fullInput({
            TargetFile: "/a.rs",
            ReplacementChunks: [{ TargetContent: "only_old" }],
          }),
        }),
      ),
    ).toBeNull();
    expect(
      buildInputDiff(
        call("c-write", "write", {
          rawName: "write_to_file",
          ...fullInput({
            TargetFile: "/a.rs",
            CodeContent: "line1\nline2\n",
          }),
        }),
      ),
    ).toBe("@@\n+line1\n+line2");
    expect(
      buildInputDiff(
        call("c-empty", "write", {
          rawName: "write_file",
          ...fullInput({ Path: "/.gitkeep", Content: "" }),
        }),
      ),
    ).toBeNull();

    renderReader("agy", [
      msg("user", [text("检查工具展示")], { turnId: "t1" }),
      msg(
        "assistant",
        [
          call("agy-edit", "edit", {
            rawName: "multi_replace_file_content",
            title: `${PROJECT}/src/main.rs`,
            inputPreview: editInput,
            inputTotalLen: editInput.length,
            diff: {
              files: [
                {
                  path: `${PROJECT}/src/main.rs`,
                  op: "update",
                  added: 2,
                  removed: 1,
                },
              ],
              added: 2,
              removed: 1,
            },
          }),
          call("agy-write-trunc", "write", {
            rawName: "write_file",
            title: `${PROJECT}/src/big.rs`,
            inputPreview: truncatedWriteInput.slice(0, 400),
            inputTotalLen: truncatedWriteInput.length,
            diff: {
              files: [
                {
                  path: `${PROJECT}/src/big.rs`,
                  op: "add",
                  added: 40,
                  removed: 0,
                },
              ],
              added: 40,
              removed: 0,
            },
          }),
          call("agy-cmd", "shell", {
            rawName: "run_command",
            title: "export FOO=1…",
            inputPreview: multiCmdInput,
            inputTotalLen: multiCmdInput.length,
          }),
          call("agy-task", "shell", {
            rawName: "manage_task",
            title: "conv-1/task-8",
            detail: "status",
            inputPreview: taskInput,
            inputTotalLen: taskInput.length,
          }),
        ],
        { turnId: "t1" },
      ),
      msg(
        "tool",
        [
          result("agy-edit"),
          result("agy-write-trunc"),
          result("agy-cmd"),
          result("agy-task"),
        ],
        { turnId: "t1" },
      ),
    ]);

    // 1. ReplacementChunks 还原出改动内容
    fireEvent.click(stepButton(/修改 src\/main\.rs/));
    const editRegion = screen.getByRole("region", {
      name: /修改 src\/main\.rs/,
    });
    expect(within(editRegion).getByText("改动")).toBeInTheDocument();
    expect(within(editRegion).getByText("old_line")).toBeInTheDocument();
    expect(within(editRegion).getByText("new_line_2")).toBeInTheDocument();

    // 2. 单文件参数被截断且无 diff.full 时，不渲染空的「改动」标题
    fireEvent.click(stepButton(/写入 src\/big\.rs/));
    const writeRegion = screen.getByRole("region", {
      name: /写入 src\/big\.rs/,
    });
    expect(within(writeRegion).queryByText("改动")).not.toBeInTheDocument();

    // 3. <= 400 字的多行 CommandLine 展示完整代码块，且不重复展示 JSON 参数区
    fireEvent.click(stepButton(/运行 export FOO=1…/));
    const cmdRegion = screen.getByRole("region", {
      name: /运行 export FOO=1…/,
    });
    expect(
      within(cmdRegion).getByText(/cargo test --lib agy/),
    ).toBeInTheDocument();
    expect(within(cmdRegion).queryByText("参数")).not.toBeInTheDocument();

    // 4. 无命令字段的 shell 工具（manage_task）保留可展开的「参数」区
    fireEvent.click(stepButton(/运行 conv-1\/task-8/));
    const taskRegion = screen.getByRole("region", {
      name: /运行 conv-1\/task-8/,
    });
    const paramsBtn = within(taskRegion).getByRole("button", { name: "参数" });
    expect(paramsBtn).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(paramsBtn);
    expect(
      within(taskRegion).getByText(/"TaskId": "conv-1\/task-8"/),
    ).toBeInTheDocument();
  });
});
