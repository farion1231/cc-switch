import { useCallback, useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { ArrowDown } from "lucide-react";
import "@xterm/xterm/css/xterm.css";

import { terminalApi } from "@/lib/api/terminal";
import { terminalQueue } from "@/lib/terminalQueue";
import type { TerminalInstance } from "@/types/terminal";
import { useDarkMode } from "@/hooks/useDarkMode";
import { TerminalPrompt } from "./TerminalPrompt";

/** 深色终端配色（默认） */
const DARK_TERMINAL_THEME = {
  background: "#0b0f14",
  foreground: "#d4d4d8",
  cursor: "#34d399",
  selectionBackground: "#1e293b",
  black: "#000000",
  red: "#f87171",
  green: "#34d399",
  yellow: "#fbbf24",
  blue: "#60a5fa",
  magenta: "#c084fc",
  cyan: "#22d3ee",
  white: "#e4e4e7",
  brightBlack: "#52525b",
  brightRed: "#fca5a5",
  brightGreen: "#6ee7b7",
  brightYellow: "#fde047",
  brightBlue: "#93c5fd",
  brightMagenta: "#d8b4fe",
  brightCyan: "#67e8f9",
  brightWhite: "#fafafa",
};

/** 浅色终端配色（跟随应用浅色主题） */
const LIGHT_TERMINAL_THEME = {
  background: "#ffffff",
  foreground: "#3f3f46",
  cursor: "#059669",
  cursorAccent: "#ffffff",
  selectionBackground: "#bfdbfe",
  black: "#000000",
  red: "#dc2626",
  green: "#059669",
  yellow: "#b45309",
  blue: "#2563eb",
  magenta: "#7c3aed",
  cyan: "#0891b2",
  white: "#52525b",
  brightBlack: "#71717a",
  brightRed: "#ef4444",
  brightGreen: "#10b981",
  brightYellow: "#d97706",
  brightBlue: "#3b82f6",
  brightMagenta: "#8b5cf6",
  brightCyan: "#06b6d4",
  brightWhite: "#18181b",
};

export interface EmbeddedTerminalProps {
  /** 要启动的终端实例（切换实例时自动重启会话） */
  instance: TerminalInstance;
  /**
   * 终端是否可见。隐藏（display:none）期间会话与 xterm 都保持挂载，
   * 重新可见时需要重新自适应尺寸并强制重绘。
   */
  active?: boolean;
  /** 递增触发重启（重置/恢复会话用） */
  resetNonce?: number;
  /** 会话 pty 状态变化回调（供外部停止按钮使用） */
  onPtyChange?: (ptyId: number | null) => void;
  /** 进程退出状态变化回调 */
  onExitChange?: (exited: boolean) => void;
}

/**
 * 面板内嵌终端：会话由 Rust 后端持有（PTY），本组件只是连接/显示层。
 * - 卸载/断开连接不会终止进程，重新挂载时复用会话并回放输出历史；
 * - 「停止 / 重置」由外部通过 closeEmbedded 显式终止，这里相应显示已退出。
 */
export function EmbeddedTerminal({
  instance,
  active = true,
  resetNonce = 0,
  onPtyChange,
  onExitChange,
}: EmbeddedTerminalProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const ptyIdRef = useRef<number | null>(null);
  const bootGenRef = useRef(0);
  const instanceRef = useRef(instance);
  instanceRef.current = instance;

  const isDark = useDarkMode();
  const isDarkRef = useRef(isDark);
  isDarkRef.current = isDark;

  const [bootNonce, setBootNonce] = useState(0);
  const [exited, setExited] = useState(false);
  const [bootError, setBootError] = useState<string | null>(null);
  /** 是否处于非底部（滚动到上面后显示「回到底部」按钮） */
  const [showScrollToBottom, setShowScrollToBottom] = useState(false);

  // 应用主题切换时，动态更新 xterm 配色（无需重启会话）
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.theme = isDark ? DARK_TERMINAL_THEME : LIGHT_TERMINAL_THEME;
    try {
      term.refresh(0, term.rows - 1);
    } catch {
      // 终端尚未渲染完成，忽略
    }
  }, [isDark]);

  const reportPty = useCallback(
    (ptyId: number | null) => {
      ptyIdRef.current = ptyId;
      onPtyChange?.(ptyId);
    },
    [onPtyChange],
  );

  const reportExit = useCallback(
    (value: boolean) => {
      setExited(value);
      onExitChange?.(value);
    },
    [onExitChange],
  );

  /**
   * 队列发送器：往本会话写内容 + 回车。
   * 注册到队列 store 后，排队发送就绑定到这个终端（跟随终端）。
   */
  const sendText = useCallback(async (text: string) => {
    const ptyId = ptyIdRef.current;
    if (ptyId === null) {
      throw new Error("当前终端会话不可用");
    }
    // 换行统一成 \r\n：cmd/ConPTY 只认回车执行，裸 \n 会打断当前行
    const payload = text.replace(/\r\n|\r|\n/g, "\r\n");
    await terminalApi.writeEmbedded(ptyId, payload);
    await new Promise((resolve) => setTimeout(resolve, 150));
    await terminalApi.writeEmbedded(ptyId, "\r");
  }, []);

  useEffect(() => {
    terminalQueue.registerSender(instance.id, sendText);
    return () => terminalQueue.unregisterSender(instance.id);
  }, [instance.id, sendText]);

  /** 自适应终端尺寸并同步 PTY（校验 dims，避免未布局时算出非法行列）。 */
  const doFit = useCallback(() => {
    const term = termRef.current;
    const fit = fitRef.current;
    if (!term || !fit) return;
    try {
      fit.fit();
      const dims = fit.proposeDimensions();
      const ptyId = ptyIdRef.current;
      if (dims && dims.cols > 0 && dims.rows > 0 && ptyId !== null) {
        void terminalApi.resizeEmbedded(ptyId, dims.cols, dims.rows);
      }
    } catch {
      // 容器尚未布局完成，忽略
    }
  }, []);

  // 终端重新变为可见（切换标签回来）时：重新自适应尺寸并强制重绘。
  // display:none 期间容器尺寸为 0，fit 与渲染都失效，需要激活后补救。
  useEffect(() => {
    if (!active) return;
    doFit();
    const term = termRef.current;
    if (term) {
      try {
        term.refresh(0, term.rows - 1);
      } catch {
        // 终端尚未渲染完成，忽略
      }
    }
  }, [active, doFit]);

  const boot = useCallback(async () => {
    const current = instanceRef.current;
    const container = containerRef.current;
    if (!container) return;

    // 新会话默认位于底部，隐藏「回到底部」按钮（若切换/重启前停在旧内容上方）
    setShowScrollToBottom(false);

    // 代数递增：使之前未完成的异步 boot 失效（防快速切换实例时误注册）
    const gen = ++bootGenRef.current;

    // 清理上一个 boot 遗留的 xterm（StrictMode 双挂载等场景，避免实例叠加）
    if (termRef.current) {
      try {
        termRef.current.dispose();
      } catch {
        // 忽略
      }
      termRef.current = null;
      fitRef.current = null;
    }

    const term = new Terminal({
      fontSize: 12,
      fontFamily:
        'ui-monospace, "Cascadia Mono", Consolas, "Courier New", monospace',
      cursorBlink: true,
      scrollback: 5000,
      theme: isDarkRef.current ? DARK_TERMINAL_THEME : LIGHT_TERMINAL_THEME,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    termRef.current = term;
    fitRef.current = fit;

    term.open(container);
    doFit();

    // WebGL 渲染器：默认 DOM 渲染器每个单元格是独立 DOM 节点，ConPTY 在
    // 每次按键回显时整屏重绘，表现为输入内容时终端闪烁一下；WebGL 原子化
    // 绘制可消除闪烁。初始化失败（无 GPU / 上下文丢失）自动回退 DOM。
    let webgl: WebglAddon | null = null;
    try {
      webgl = new WebglAddon();
      webgl.onContextLoss(() => {
        webgl?.dispose();
      });
      term.loadAddon(webgl);
    } catch {
      webgl?.dispose();
    }

    const disposables: Array<() => void> = [];
    const onDataDisposable = term.onData((data) => {
      const ptyId = ptyIdRef.current;
      if (ptyId !== null) {
        void terminalApi.writeEmbedded(ptyId, data);
      }
    });
    disposables.push(() => onDataDisposable.dispose());
    // 滚动监听：离开底部时显示「回到底部」按钮，回到底部自动隐藏
    const onScrollDisposable = term.onScroll(() => {
      const buffer = term.buffer.active;
      setShowScrollToBottom(buffer.viewportY < buffer.baseY);
    });
    disposables.push(() => onScrollDisposable.dispose());

    // 备用缓冲区滚轮桥接（alternate-scroll）：opencode 等全屏 TUI 运行在
    // 备用缓冲区，没有回滚可滚；当应用未启用鼠标追踪时，把滚轮翻译成
    // 方向键写入 PTY（与 Windows Terminal 的默认行为一致）。
    // 返回 false 让 xterm 跳过自身处理；普通缓冲区与鼠标感知 TUI 不受影响。
    term.attachCustomWheelEventHandler((ev) => {
      if (term.buffer.active.type !== "alternate") return true;
      if (term.modes.mouseTrackingMode !== "none") return true;
      ev.preventDefault();
      // deltaMode 1 = 行；否则按像素（约 40px 一行）折算，单次 1~8 行
      const delta = ev.deltaMode === 1 ? ev.deltaY * 16 : ev.deltaY;
      const lines = Math.max(
        1,
        Math.min(8, Math.round(Math.abs(delta) / 40) || 1),
      );
      // DECCKM（应用光标键模式）开启时用 SS3 序列
      const cc = term.modes.applicationCursorKeysMode ? "O" : "[";
      const seq = (delta < 0 ? `\x1b${cc}A` : `\x1b${cc}B`).repeat(lines);
      const ptyId = ptyIdRef.current;
      if (ptyId !== null) {
        void terminalApi.writeEmbedded(ptyId, seq);
      }
      return false;
    });

    setBootError(null);
    reportExit(false);
    try {
      // 幂等：复用已有会话（后端保持运行），仅当会话不存在/已退出时新建
      const ptyId = await terminalApi.ensureEmbedded({
        instanceId: current.id,
        app: current.app,
        projectDir: current.projectDir,
        tool: current.tool,
        customCommand: current.customCommand ?? null,
        args: current.args ?? null,
        terminal: current.terminal ?? null,
        providerId: null,
      });
      // 启动期间实例已被切换：本次连接作废（会话本身保留在后台）
      if (gen !== bootGenRef.current) {
        try {
          term.dispose();
        } catch {
          // 忽略
        }
        return;
      }
      reportPty(ptyId);
      await terminalApi.attachEmbedded(ptyId, (evt) => {
        if (evt.kind === "data") {
          term.write(new Uint8Array(evt.data));
          // 告知队列「本终端有输出」：排队发送靠输出静默判定上一条是否执行完
          terminalQueue.notifyOutput(instance.id);
        } else {
          reportExit(true);
        }
      });
      // 历史回放完成后滚到最新内容（xterm 通常自动滚动，这里兜底）
      try {
        term.scrollToBottom();
      } catch {
        // 忽略
      }
      // 尺寸变化 → 自适应并同步 PTY
      const resizeObserver = new ResizeObserver(() => doFit());
      resizeObserver.observe(container);
      disposables.push(() => resizeObserver.disconnect());
      // 布局/字体就绪后再自适应几次（首次 fit 常因字体未加载而偏小）
      const fitTimers = [
        window.setTimeout(doFit, 50),
        window.setTimeout(doFit, 300),
      ];
      if (document.fonts?.ready) {
        document.fonts.ready.then(doFit).catch(() => undefined);
      }
      disposables.push(() => {
        fitTimers.forEach((timer) => window.clearTimeout(timer));
      });
    } catch (error) {
      console.error("[EmbeddedTerminal] failed to connect pty", error);
      setBootError(String(error));
      reportExit(true);
    }

    return () => {
      disposables.forEach((fn) => fn());
    };
  }, [reportExit, reportPty]);

  // 实例切换 / 外部触发（重置、恢复会话）→ 重新连接（复用或新建会话）
  useEffect(() => {
    let cleanup: (() => void) | undefined;
    let cancelled = false;
    void boot().then((fn) => {
      if (cancelled) {
        fn?.();
      } else {
        cleanup = fn;
      }
    });
    return () => {
      cancelled = true;
      bootGenRef.current += 1;
      cleanup?.();
      // 切换实例时同步清空 pty 关联：底部输入框（TerminalPrompt）读的是
      // 这个 ref，不清空会继续写到上一个会话；新会话连接成功后由
      // reportPty 恢复。不调用 reportPty(null)——后台会话仍然存活，
      // 不能误报 detached 状态。
      ptyIdRef.current = null;
      // 清理本实例的 xterm DOM；会话保留在后台继续运行（重连即可恢复）
      if (termRef.current) {
        try {
          termRef.current.dispose();
        } catch {
          // 忽略
        }
        termRef.current = null;
        fitRef.current = null;
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [instance.id, resetNonce, bootNonce]);

  /** 显式重启：终止当前会话并启动全新会话（后端 kill 旧进程）。 */
  const restart = useCallback(async () => {
    const oldPty = ptyIdRef.current;
    if (oldPty !== null) {
      try {
        await terminalApi.closeEmbedded(oldPty);
      } catch {
        // 忽略
      }
      reportPty(null);
    }
    setBootNonce((n) => n + 1);
  }, [reportPty]);

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      {/* xterm 容器绝对定位铺满：避免 flex 布局干扰 xterm 的绝对定位 viewport */}
      <div className="relative min-h-0 flex-1">
        <div ref={containerRef} className="absolute inset-0" />
        {/* 回到底部按钮：仅当向上滚动离开底部时显示 */}
        {showScrollToBottom && (
          <button
            type="button"
            onClick={() => {
              termRef.current?.scrollToBottom();
              setShowScrollToBottom(false);
            }}
            title="滚动到底部"
            aria-label="滚动到底部"
            className="absolute bottom-3 right-3 z-10 flex h-7 w-7 items-center justify-center rounded-full border border-border bg-background/90 text-muted-foreground shadow-md transition-colors hover:bg-muted hover:text-foreground"
          >
            <ArrowDown className="h-3.5 w-3.5" />
          </button>
        )}
      </div>
      <TerminalPrompt instanceId={instance.id} getPtyId={() => ptyIdRef.current} />
      {(exited || bootError) && (
        <div className="flex shrink-0 items-center justify-center gap-2 border-t border-border py-2 text-[11px] text-muted-foreground">
          <span>
            {bootError
              ? `内嵌终端启动失败：${bootError}`
              : "终端进程已退出，输出保留在下方"}
          </span>
          <button
            type="button"
            onClick={restart}
            className="rounded border border-border px-2 py-0.5 text-muted-foreground transition-colors hover:bg-muted"
          >
            重新启动
          </button>
        </div>
      )}
    </div>
  );
}
