import { useMemo, useState, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { motion, AnimatePresence } from "framer-motion";
import { Zap, ChevronDown } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useUsageSummaryByApp } from "@/lib/query/usage";
import { useUsageEventBridge } from "@/hooks/useUsageEventBridge";
import { useUsageCacheBridge } from "@/hooks/useUsageCacheBridge";
import { DRAG_REGION_ENABLED } from "@/lib/platform";
import { fmtUsd, formatTokensShort, getResolvedLang } from "./format";

export function FloatingUsageWindow() {
  const { t, i18n } = useTranslation();
  const lang = getResolvedLang(i18n);
  const [isExpanded, setIsExpanded] = useState(true);

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

  // 隐藏窗口背景色以实现透明悬浮效果
  useEffect(() => {
    document.body.style.backgroundColor = "transparent";
    document.documentElement.style.backgroundColor = "transparent";
    return () => {
      document.body.style.backgroundColor = "";
      document.documentElement.style.backgroundColor = "";
    };
  }, []);

  // 展开/收起时通过 Rust 后端调整窗口尺寸（JS setSize 在 Windows 透明窗口上静默失效）
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout>;
    const updateSize = (height: number) => {
      invoke("resizeFloatingUsageWindow", { width: 220, height }).catch((e) =>
        console.error("Failed to resize floating window", e),
      );
    };

    if (isExpanded) {
      updateSize(300);
    } else {
      timer = setTimeout(() => updateSize(64), 200);
    }

    return () => {
      if (timer) clearTimeout(timer);
    };
  }, [isExpanded]);

  // 订阅 Tauri IPC 事件以进行数据自动更新
  useUsageEventBridge();
  useUsageCacheBridge();

  const range = useMemo(() => ({ preset: "today" as const }), []);
  const { data } = useUsageSummaryByApp(range);

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
    const cacheableInput = input + cacheRead;
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
    <div className="w-full h-full min-h-screen flex flex-col items-center justify-start p-1 select-none bg-transparent">
      <div
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
        <AnimatePresence>
          {isExpanded && (
            <motion.div
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
