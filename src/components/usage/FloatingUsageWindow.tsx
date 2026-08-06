import { useMemo, useState, useEffect, useRef, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { motion, AnimatePresence } from "framer-motion";
import { Zap, ChevronDown } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, PhysicalPosition } from "@tauri-apps/api/window";
import { useUsageSummaryByApp } from "@/lib/query/usage";
import { useUsageEventBridge } from "@/hooks/useUsageEventBridge";
import { useUsageCacheBridge } from "@/hooks/useUsageCacheBridge";
import { DRAG_REGION_ENABLED } from "@/lib/platform";
import { fmtUsd, formatTokensShort, getResolvedLang } from "./format";
import type { Settings } from "@/types";

// 悬浮窗宽度常量 —— 需与 src-tauri/tauri.conf.json、tauri.windows.conf.json
// 中 floating_usage 窗口的 width 保持一致（config 为 JSON 无法引用此常量）。
// 窗口高度由内容动态测量决定，不再与 config 绑定。
const FLOATING_WINDOW_WIDTH = 220;
// 首次展开时的兜底高度：展开动画前需要先把窗口撑到足够容纳明细区，
// 否则动画期间内容会被窗口裁切。展开结束后会用实测高度覆盖，此值不会残留。
const FLOATING_WINDOW_EXPANDED_FALLBACK_HEIGHT = 320;
// 悬浮窗位置持久化 key（localStorage）。拖拽移动时记录物理坐标，下次启动时恢复。
// 悬浮窗被 window-state 插件 denylist 排除（避免恢复旧尺寸），位置需自行持久化。
const FLOATING_WINDOW_POSITION_KEY = "floating_usage_position";

// 恢复上次记录的位置（物理坐标）。数据缺失或 setPosition 失败时保持系统默认位置。
async function restoreWindowPosition(win: ReturnType<typeof getCurrentWindow>) {
  try {
    const raw = localStorage.getItem(FLOATING_WINDOW_POSITION_KEY);
    if (!raw) return;
    const { x, y } = JSON.parse(raw) as { x?: unknown; y?: unknown };
    if (typeof x === "number" && typeof y === "number") {
      await win.setPosition(new PhysicalPosition(x, y));
    }
  } catch (e) {
    console.error("Failed to restore floating window position", e);
  }
}

export function FloatingUsageWindow() {
  const { t, i18n } = useTranslation();
  const lang = getResolvedLang(i18n);
  const [isExpanded, setIsExpanded] = useState(() => {
    try {
      const saved = localStorage.getItem("floating_usage_expanded");
      return saved !== null ? saved === "true" : true;
    } catch {
      return true;
    }
  });

  const cardRef = useRef<HTMLDivElement>(null);
  const isFirstRenderRef = useRef(true);

  // 记录最近一次展开时测得的窗口高度，供展开动画前预撑窗口使用。
  // 避免每次展开都先撑到固定大高度（如 500）造成明显的透明大窗闪一下；
  // 展开后 syncWindowHeight(false) 会精确贴合真实高度，兜底值不会残留。
  const expandedHeightRef = useRef(FLOATING_WINDOW_EXPANDED_FALLBACK_HEIGHT);
  const isExpandedRef = useRef(isExpanded);
  useEffect(() => {
    isExpandedRef.current = isExpanded;
  }, [isExpanded]);

  const syncWindowHeight = useCallback((isExpanding: boolean) => {
    const cardEl = cardRef.current;
    if (!cardEl) return;

    if (isExpanding) {
      // 展开动画前先把窗口撑到上次记录的展开高度（兜底 320 已足够容纳明细区），
      // 这样动画全程内容不会被窗口裁切；动画结束后会再精确贴合。
      invoke("resizeFloatingUsageWindow", {
        width: FLOATING_WINDOW_WIDTH,
        height: expandedHeightRef.current,
      }).catch(console.error);
    } else {
      const rect = cardEl.getBoundingClientRect();
      const targetHeight = Math.max(40, Math.ceil(rect.height));
      if (isExpandedRef.current) {
        expandedHeightRef.current = targetHeight;
      }
      invoke("resizeFloatingUsageWindow", {
        width: FLOATING_WINDOW_WIDTH,
        height: targetHeight,
      }).catch(console.error);
    }
  }, []);

  // 鼠标拖拽状态管理
  const [dragStart, setDragStart] = useState<{ x: number; y: number } | null>(
    null,
  );
  const hasDraggedRef = useRef(false);

  const handleMouseDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return; // 仅限左键
    setDragStart({ x: e.screenX, y: e.screenY });
    hasDraggedRef.current = false;
  };

  const handleMouseMove = (e: React.MouseEvent) => {
    if (!dragStart || !DRAG_REGION_ENABLED) return;
    const dx = e.screenX - dragStart.x;
    const dy = e.screenY - dragStart.y;
    const dist = Math.sqrt(dx * dx + dy * dy);
    if (dist > 5) {
      setDragStart(null);
      hasDraggedRef.current = true;
      getCurrentWindow()
        .startDragging()
        .catch((err) => {
          // 拖拽启动失败（如平台不支持 / 权限缺失）时复位标记，
          // 避免后续点击被 hasDraggedRef 误判为"刚拖过"而无法展开/收起
          hasDraggedRef.current = false;
          console.error("Failed to start window dragging", err);
        });
    }
  };

  const handleMouseUp = () => {
    setDragStart(null);
  };

  // 拖拽移动窗口后记录位置，供下次启动恢复
  useEffect(() => {
    const win = getCurrentWindow();
    let unlisten: (() => void) | undefined;
    let saveTimer: ReturnType<typeof setTimeout> | undefined;

    win
      .onMoved(({ payload }) => {
        // 移动事件频率较高，做简单防抖再落盘
        clearTimeout(saveTimer);
        saveTimer = setTimeout(() => {
          try {
            localStorage.setItem(
              FLOATING_WINDOW_POSITION_KEY,
              JSON.stringify({ x: payload.x, y: payload.y }),
            );
          } catch (e) {
            console.error("Failed to save floating window position", e);
          }
        }, 200);
      })
      .then((off) => {
        unlisten = off;
      });

    return () => {
      clearTimeout(saveTimer);
      unlisten?.();
    };
  }, []);

  // 订阅 Tauri IPC 事件以进行数据自动更新
  useUsageEventBridge();
  useUsageCacheBridge();

  const range = useMemo(() => ({ preset: "today" as const }), []);
  const { data, isLoading } = useUsageSummaryByApp(range);

  // 保存展开/收起偏好至 localStorage
  useEffect(() => {
    try {
      localStorage.setItem("floating_usage_expanded", String(isExpanded));
    } catch (e) {
      console.error("Failed to save floating_usage_expanded", e);
    }
  }, [isExpanded]);

  // 隐藏窗口背景色以实现透明悬浮效果，并在渲染就绪后显示窗口
  useEffect(() => {
    document.body.style.backgroundColor = "transparent";
    document.documentElement.style.backgroundColor = "transparent";

    if (!isLoading) {
      invoke<Settings>("get_settings")
        .then(async (settings) => {
          if (settings && settings.enableFloatingUsage) {
            const win = getCurrentWindow();
            // 先恢复上次记录的位置，再显示窗口，避免窗口先出现在默认位置再跳变
            await restoreWindowPosition(win);
            // 等待浏览器实际绘制完成后再显示窗口，彻底消除白屏闪烁
            requestAnimationFrame(() => {
              win
                .show()
                .then(() => {
                  requestAnimationFrame(() => syncWindowHeight(false));
                })
                .catch((e) =>
                  console.error("Failed to show floating window", e),
                );
            });
          }
        })
        .catch(() => {
          // IPC 拉取设置失败时保持隐藏，避免误弹出悬浮窗
        });
    }

    return () => {
      document.body.style.backgroundColor = "";
      document.documentElement.style.backgroundColor = "";
    };
  }, [isLoading, syncWindowHeight]);

  // 展开/收起时通过 Rust 后端调整窗口尺寸
  useEffect(() => {
    if (isFirstRenderRef.current) {
      isFirstRenderRef.current = false;
      return;
    }

    let timer: ReturnType<typeof setTimeout>;
    if (isExpanded) {
      // 展开时：立刻撑大窗口防止动画被裁切
      syncWindowHeight(true);
      // 动画完成后（约200ms）再精确贴合
      timer = setTimeout(() => syncWindowHeight(false), 250);
    } else {
      // 收起时：先播放动画，动画完成后再缩紧窗口，避免裁切
      timer = setTimeout(() => syncWindowHeight(false), 200);
    }

    return () => {
      if (timer) clearTimeout(timer);
    };
  }, [isExpanded, syncWindowHeight]);

  // 非动画期间的内容高度变化（如 i18n 切换、数据加载）
  useEffect(() => {
    const cardEl = cardRef.current;
    if (!cardEl) return;
    let observerTimer: ReturnType<typeof setTimeout>;
    const observer = new ResizeObserver(() => {
      clearTimeout(observerTimer);
      observerTimer = setTimeout(() => {
        syncWindowHeight(false);
      }, 150);
    });
    observer.observe(cardEl);
    return () => {
      observer.disconnect();
      clearTimeout(observerTimer);
    };
  }, [syncWindowHeight]);

  // 累加今日所有应用的 Token 用量与总成本及明细
  const summary = useMemo(() => {
    if (!data || data.length === 0) {
      return {
        totalCost: 0,
        realTotalTokens: 0,
        inputTokens: 0,
        outputTokens: 0,
        cacheCreationTokens: 0,
        cacheReadTokens: 0,
        cacheHitRate: 0,
      };
    }
    let totalCostNum = 0;
    let input = 0;
    let output = 0;
    let cacheCreation = 0;
    let cacheRead = 0;

    for (const item of data) {
      totalCostNum += parseFloat(item.summary.totalCost) || 0;
      input += item.summary.totalInputTokens || 0;
      output += item.summary.totalOutputTokens || 0;
      cacheCreation += item.summary.totalCacheCreationTokens || 0;
      cacheRead += item.summary.totalCacheReadTokens || 0;
    }

    const realTotal = input + output + cacheCreation + cacheRead;
    const cacheableInput = input + cacheCreation + cacheRead;
    const cacheHitRate =
      cacheableInput > 0 ? (cacheRead / cacheableInput) * 100 : 0;

    return {
      totalCost: totalCostNum,
      realTotalTokens: realTotal,
      inputTokens: input,
      outputTokens: output,
      cacheCreationTokens: cacheCreation,
      cacheReadTokens: cacheRead,
      cacheHitRate,
    };
  }, [data]);

  const formattedCost =
    summary.totalCost >= 0.1 || summary.totalCost === 0
      ? fmtUsd(summary.totalCost, 2)
      : fmtUsd(summary.totalCost, 4);

  return (
    <div className="w-full h-fit flex flex-col items-center justify-start select-none bg-transparent">
      <div
        ref={cardRef}
        onMouseDown={handleMouseDown}
        onMouseMove={handleMouseMove}
        onMouseUp={handleMouseUp}
        onMouseLeave={handleMouseUp}
        className="group relative flex flex-col w-full p-2.5 rounded-xl bg-background/90 dark:bg-background/85 backdrop-blur-md border border-border/50 text-foreground shadow-lg cursor-move"
      >
        {/* 顶部简要信息区域 */}
        <div
          onClick={() => {
            if (hasDraggedRef.current) return;
            setIsExpanded(!isExpanded);
          }}
          className="flex items-center justify-between w-full cursor-pointer"
        >
          <div className="flex items-center gap-2 pointer-events-none min-w-0">
            <div className="p-1.5 rounded-lg bg-emerald-500/10 text-emerald-500 shrink-0">
              <Zap className="h-4 w-4" />
            </div>
            <div className="flex flex-col min-w-0">
              <span className="text-[10px] text-muted-foreground font-medium uppercase tracking-wider leading-none mb-0.5">
                {t("usage.presetToday", "Today")}
              </span>
              <div className="flex items-baseline gap-1.5 leading-none whitespace-nowrap">
                <span className="text-sm font-bold tabular-nums">
                  {formatTokensShort(summary.realTotalTokens, lang)}
                </span>
                <span className="text-[10px] text-muted-foreground font-medium">
                  ({formattedCost})
                </span>
              </div>
            </div>
          </div>

          {/* 展开/收起切换按钮 */}
          <button
            onMouseDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation();
              setIsExpanded(!isExpanded);
            }}
            className="opacity-0 group-hover:opacity-100 transition-all duration-200 p-1 rounded-md hover:bg-muted/60 text-muted-foreground hover:text-foreground cursor-pointer shrink-0 ml-1"
          >
            <ChevronDown
              className={`h-3.5 w-3.5 transition-transform duration-200 ${
                isExpanded ? "rotate-180" : ""
              }`}
            />
          </button>
        </div>

        {/* 展开的明细数据区域 */}
        <AnimatePresence initial={false}>
          {isExpanded && (
            <motion.div
              key="expanded-content"
              onMouseDown={(e) => e.stopPropagation()}
              initial={{ opacity: 0, height: 0 }}
              animate={{ opacity: 1, height: "auto" }}
              exit={{ opacity: 0, height: 0 }}
              transition={{ duration: 0.2 }}
              className="overflow-hidden"
            >
              <div className="pt-2.5 mt-2 border-t border-border/30 grid grid-cols-2 gap-1.5 text-xs">
                <div className="flex flex-col bg-muted/30 p-1.5 rounded-md">
                  <span className="text-[10px] text-muted-foreground">
                    {t("usage.freshInput", "Input")}
                  </span>
                  <span className="font-semibold tabular-nums text-blue-500">
                    {formatTokensShort(summary.inputTokens, lang)}
                  </span>
                </div>
                <div className="flex flex-col bg-muted/30 p-1.5 rounded-md">
                  <span className="text-[10px] text-muted-foreground">
                    {t("usage.output", "Output")}
                  </span>
                  <span className="font-semibold tabular-nums text-purple-500">
                    {formatTokensShort(summary.outputTokens, lang)}
                  </span>
                </div>
                <div className="flex flex-col bg-muted/30 p-1.5 rounded-md">
                  <span className="text-[10px] text-muted-foreground">
                    {t("usage.cacheWrite", "Creation")}
                  </span>
                  <span className="font-semibold tabular-nums text-amber-500">
                    {formatTokensShort(summary.cacheCreationTokens, lang)}
                  </span>
                </div>
                <div className="flex flex-col bg-muted/30 p-1.5 rounded-md">
                  <span className="text-[10px] text-muted-foreground">
                    {t("usage.cacheRead", "Hit")}
                  </span>
                  <span className="font-semibold tabular-nums text-emerald-500">
                    {formatTokensShort(summary.cacheReadTokens, lang)}
                  </span>
                </div>
                <div className="col-span-2 bg-muted/30 p-1.5 rounded-md">
                  <div className="flex items-center justify-between mb-1">
                    <span className="text-[10px] text-muted-foreground">
                      {t("usage.cacheHitRate", "Cache Hit Rate")}
                    </span>
                    <span className="font-semibold tabular-nums text-emerald-500 text-[11px]">
                      {summary.cacheHitRate.toFixed(
                        summary.cacheHitRate >= 99.95 ? 0 : 1,
                      )}
                      %
                    </span>
                  </div>
                  <div className="relative h-1 rounded-full bg-muted/60 overflow-hidden">
                    <motion.div
                      className="absolute inset-y-0 left-0 bg-emerald-500 rounded-full"
                      initial={{ width: 0 }}
                      animate={{
                        width: `${Math.max(0, Math.min(100, summary.cacheHitRate))}%`,
                      }}
                      transition={{ duration: 0.6, ease: "easeOut" }}
                    />
                  </div>
                </div>
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </div>
  );
}
