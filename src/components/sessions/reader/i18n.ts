import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import type { Translate } from "./toolSummary";

/**
 * 阅读页文案（`sessionManager.reader.*`）。
 * 这里的简体中文是各 key 的 defaultValue：语言包缺 key（含单测）时仍有可读文案。
 * 四套语言包里的取值以 src/i18n/locales/*.json 为准。
 */
export const READER_I18N_PREFIX = "sessionManager.reader.";

export const READER_DEFAULTS: Record<string, string> = {
  timelineSummary: "执行过程 · {{steps}} 步",
  summaryCommands: "{{count}} 个命令",
  summaryFiles: "改了 {{count}} 个文件",
  summaryErrors: "{{count}} 个失败",
  summaryDuration: "⏱ {{duration}}",
  summaryTokens: "{{tokens}} tok",
  expandTimeline: "展开执行过程",
  collapseTimeline: "收起执行过程",
  expandAllTimelines: "展开全部过程",
  moreFailures: "还有 {{count}} 个失败",
  stepExpand: "展开 {{title}} 的详情",
  stepCollapse: "收起",
  "status.success": "成功",
  "status.error": "失败",
  "status.interrupted": "已中断",
  "status.pending": "进行中",
  "status.unknown": "未知",
  exitCode: "exit {{code}}",
  lines: "{{count}} 行",
  moreLines: "还有 {{count}} 行",
  showAll: "显示全部",
  loadMore: "加载更多",
  tooLarge: "内容超过 2 MB，请复制源文件路径后在本地查看",
  loading: "加载中…",
  loadFailed: "无法读取完整内容",
  params: "参数",
  output: "输出",
  diff: "改动",
  "thinking.claude": "思考中…",
  "thinking.codex": "推理",
  "thinking.gemini": "想法",
  "thinking.opencode": "Thought",
  "thinking.pi": "Thinking…",
  "thinking.generic": "思考",
  thinkingMeta: "{{chars}} 字 · {{duration}}",
  thinkingChars: "{{chars}} 字",
  thinkingRedacted: "思考内容不可见",
  note: "说明",
  noteExpand: "展开说明",
  noFinalReply: "这一轮没有最终回复",
  turnAborted: "已中断",
  "event.compaction": "上下文已压缩",
  "event.modelChange": "模型切换为 {{model}}",
  "event.thinkingLevel": "思考强度：{{level}}",
  "event.hookError": "Hook 执行出错",
  "event.prLink": "已关联 PR #{{number}}",
  "event.prLinkPlain": "已关联 PR",
  "event.slashCommand": "运行了 {{command}}",
  "event.subAgent": "子代理 {{name}}",
  injectedToggle: "显示注入的上下文",
  injectedRow: "上下文 · {{label}} · {{chars}} 字",
  "image.alt": "图 {{index}}",
  "image.open": "放大查看",
  "image.close": "关闭图片",
  "image.copy": "复制图片",
  "image.copied": "已复制图片",
  "image.failed": "无法加载图片",
  "image.loadRemote": "加载远程图片",
  "image.fit": "适应窗口",
  "image.actualSize": "原始大小",
  "link.openExternal": "在浏览器中打开 {{url}}",
  "path.copy": "复制路径",
  "path.reveal": "在 Finder 中显示",
  "merged.explored": "查看了 {{files}} 个文件、搜索 {{searches}} 次",
  "merged.mcpTimes": "调用 {{server}} {{count}} 次",
  turnsCount: "{{turns}} 轮 · {{messages}} 条",
  questionsCount: "{{count}} 次提问",
  toolsCount: "{{count}} 次工具调用",
  "usage.tokens": "{{tokens}} Tokens",
  "usage.detail":
    "{{requests}} 次请求 · 按 API 价估算\n输入 {{input}} · 输出 {{output}}\n缓存写入 {{cacheWrite}} · 缓存读取 {{cacheRead}}",
  loadingProgress: "已加载 {{loaded}} / {{total}}",
  "verb.run": "运行",
  "verb.read": "读取",
  "verb.search": "搜索",
  "verb.edit": "修改",
  "verb.write": "写入",
  "verb.web": "网页",
  "verb.mcp": "MCP",
  "verb.agent": "子代理",
  "verb.ask": "提问",
  "verb.todo": "待办",
  "verb.other": "工具",
  "verb.output": "工具输出",
  copyTurn: "复制这一轮",
  turnCopied: "已复制这一轮",
  copyWithThinking: "复制时包含思考",
  filterAll: "全部",
  filterChat: "对话",
  filterChanges: "改动",
  filterLabel: "视图",
  exportMarkdown: "导出为 Markdown 文件",
  exportShort: "导出",
  exported: "已导出到 {{path}}",
  exportFailed: "导出失败：{{error}}",
  conversationRegion: "对话内容",
  outlineTitle: "对话目录",
  outlineToggle: "对话目录",
  outlineEmptyQuestion: "（只有图片）",
  outlineSteps: "{{count}} 步",
  outlineFailed: "有失败步骤",
  outlineAborted: "已中断",
  outlineNoReply: "没有回复",
  viewOptions: "显示选项",
  noChanges: "这个会话没有文件改动",
  rawMatch: "原文中的匹配",
  expandContent: "展开完整内容",
  collapseContent: "收起",
};

/** 完整 key → defaultValue（formatStepTitle 等纯函数传进来的是完整 key） */
const defaultFor = (key: string) =>
  key.startsWith(READER_I18N_PREFIX)
    ? READER_DEFAULTS[key.slice(READER_I18N_PREFIX.length)]
    : undefined;

/**
 * 阅读页的翻译函数：`rt("status.error")` 取 `sessionManager.reader.status.error`；
 * 以 `sessionManager.` 开头的完整 key 原样使用。自动带上中文 defaultValue。
 */
export const useReaderT = (): Translate => {
  const { t } = useTranslation();
  return useCallback<Translate>(
    (key, options) => {
      const full = key.startsWith("sessionManager.")
        ? key
        : `${READER_I18N_PREFIX}${key}`;
      return t(full, { defaultValue: defaultFor(full) ?? key, ...options });
    },
    [t],
  );
};
