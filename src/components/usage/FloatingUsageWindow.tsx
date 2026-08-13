import { useMemo, useState, useEffect, useRef, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { motion, AnimatePresence } from "framer-motion";
import { Zap, ChevronDown, X } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import {
  getCurrentWindow,
  PhysicalPosition,
  cursorPosition,
} from "@tauri-apps/api/window";
import { useUsageSummaryByApp, useFloatingModelStats } from "@/lib/query/usage";
import { useUsageEventBridge } from "@/hooks/useUsageEventBridge";
import { useUsageCacheBridge } from "@/hooks/useUsageCacheBridge";
import {
  fmtUsd,
  formatTokensShort,
  computeFloatingUsageSummary,
  getResolvedLang,
} from "./format";
import type { Settings } from "@/types";

// 悬浮窗宽度常量 —— 需与 Rust 端 ensure_floating_usage_window 动态创建窗口时的
// inner_size(220.0, 64.0) 保持一致（浮窗由 Rust 动态创建，tauri.conf.json /
// tauri.windows.conf.json 中并没有 floating_usage 定义，config 为 JSON 也无法
// 引用此常量）。窗口高度由内容动态测量决定，不再与配置绑定。
const FLOATING_WINDOW_WIDTH = 220;
// 首次展开时的兜底高度：展开动画前需要先把窗口撑到足够容纳明细区，
// 否则动画期间内容会被窗口裁切。展开结束后会用实测高度覆盖，此值不会残留。
const FLOATING_WINDOW_EXPANDED_FALLBACK_HEIGHT = 320;
// 悬浮窗位置持久化 key（localStorage）。拖拽移动时记录物理坐标，下次启动时恢复。
// 悬浮窗被 window-state 插件 denylist 排除（避免恢复旧尺寸），位置需自行持久化。
const FLOATING_WINDOW_POSITION_KEY = "floating_usage_position";
// 吸附边持久化 key（localStorage）。贴边隐藏后记住吸附方向，重启后按原边恢复隐藏态。
const FLOATING_WINDOW_DOCK_KEY = "floating_usage_dock_edge";
// 贴边吸附常量（逻辑像素，最终换算成物理像素参与计算）：
const SNAP_THRESHOLD = 12; // 距工作区边缘多少逻辑 px 内触发吸附
// 吸附隐藏后露出的"一条线"宽度/高度（逻辑 px）：无论窗口是贴到边缘触发吸附，
// 还是拖出一部分漏出屏外触发吸附，吸附后都统一缩成这么细的一条线
const DOCK_PEEK_MIN = 6;
const DOCK_REVEAL_DELAY = 1000; // 鼠标移开后自动收回的延迟（ms）
// 悬停滑出判定时，光标距窗口外接矩形额外放宽的距离（物理 px），便于命中窄条
const DOCK_HOVER_MARGIN = 10;
// 吸附隐藏态轮询全局光标位置的间隔（ms）
const DOCK_HOVER_POLL_INTERVAL = 80;
// 刚隐藏成一条线后的宽限期（ms）：此期间忽略悬停轮询，避免"刚藏回去又弹出来"
const DOCK_HOVER_GRACE = 600;

type DockEdge = "left" | "right" | "top" | "bottom";
type WorkArea = { x: number; y: number; width: number; height: number };

function readDockState(): DockEdge | null {
  try {
    const v = localStorage.getItem(FLOATING_WINDOW_DOCK_KEY);
    return v === "left" || v === "right" || v === "top" || v === "bottom"
      ? v
      : null;
  } catch {
    return null;
  }
}

function saveDockState(edge: DockEdge | null) {
  try {
    if (edge) {
      localStorage.setItem(FLOATING_WINDOW_DOCK_KEY, edge);
    } else {
      localStorage.removeItem(FLOATING_WINDOW_DOCK_KEY);
    }
  } catch (e) {
    console.error("Failed to save floating window dock state", e);
  }
}

// 恢复上次记录的位置（物理坐标）。数据缺失或 setPosition 失败时保持系统默认位置。
// 返回恢复成功后的位置（供吸附隐藏态计算隐藏坐标使用）。
async function restoreWindowPosition(
  win: ReturnType<typeof getCurrentWindow>,
): Promise<{ x: number; y: number } | null> {
  try {
    const raw = localStorage.getItem(FLOATING_WINDOW_POSITION_KEY);
    if (!raw) return null;
    const { x, y } = JSON.parse(raw) as { x?: unknown; y?: unknown };
    if (typeof x === "number" && typeof y === "number") {
      await win.setPosition(new PhysicalPosition(x, y));
      return { x, y };
    }
  } catch (e) {
    console.error("Failed to restore floating window position", e);
  }
  return null;
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
  // 窗口是否已完成首次"恢复位置并显示"。防止后续数据刷新（isLoading 短暂变
  // true）时重复 restoreWindowPosition，把用户拖动后的窗口移回旧位置。
  const hasShownRef = useRef(false);

  // 记录最近一次展开时测得的窗口高度，供展开动画前预撑窗口使用。
  // 避免每次展开都先撑到固定大高度（如 500）造成明显的透明大窗闪一下；
  // 展开后 syncWindowHeight(false) 会精确贴合真实高度，兜底值不会残留。
  const expandedHeightRef = useRef(FLOATING_WINDOW_EXPANDED_FALLBACK_HEIGHT);
  const isExpandedRef = useRef(isExpanded);
  useEffect(() => {
    isExpandedRef.current = isExpanded;
  }, [isExpanded]);

  const [isFlipped, setIsFlipped] = useState(false);
  const [currentFace, setCurrentFace] = useState<"front" | "back">("front");

  // 贴边吸附状态：
  // dockEdgeRef —— 当前吸附的边（null 表示未吸附）；
  // dockedPeekedRef —— 吸附后是否已隐藏成一条线（false 表示已滑出显示）；
  // revealedPosRef —— 吸附时记录的"滑出位置"，隐藏/滑出都以此为准。
  const [dockEdge, setDockEdge] = useState<DockEdge | null>(null);
  const dockEdgeRef = useRef<DockEdge | null>(null);
  const [dockedPeeked, setDockedPeeked] = useState(false);
  const dockedPeekedRef = useRef(false);
  const revealedPosRef = useRef<{ x: number; y: number } | null>(null);

  // 拖拽结束判定：native 拖拽期间前端收不到 mouseup，改用"最后一次移动事件
  // 距今超过阈值"来推断拖拽已结束，再统一做吸附评估。
  const dragActiveRef = useRef(false);
  const dragMovedRef = useRef(false);
  const dragLastMoveAtRef = useRef(0);
  const dragSettleTimerRef = useRef<ReturnType<typeof setInterval> | null>(
    null,
  );
  const hideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const revealClickGuardRef = useRef(0);
  const animTokenRef = useRef(0);
  // 从吸附隐藏态（一条线）发起的交互：true=按下那条边开始；evaluateSnap 时
  // 据此区分"纯点击（→滑出）"与"拖出（→正常吸附评估）"
  const peekedDragRef = useRef(false);
  // 光标轮询所需的窗口几何缓存（物理坐标，onMoved / onResized 持续更新）
  const outerPosRef = useRef({ x: 0, y: 0 });
  const outerSizeRef = useRef({ width: 0, height: 0 });
  const revealPollRef = useRef<ReturnType<typeof setInterval> | null>(null);
  // 进入隐藏态的时间戳（物理），供悬停轮询的宽限期判断
  const peekedAtRef = useRef(0);

  // 获取悬浮窗所在显示器的工作区（物理坐标，排除任务栏）
  const getWorkArea = useCallback(async (): Promise<WorkArea | null> => {
    try {
      return await invoke<WorkArea | null>("get_floating_work_area");
    } catch (e) {
      console.error("Failed to get floating window work area", e);
      return null;
    }
  }, []);

  const syncWindowHeight = useCallback(
    (isExpanding: boolean) => {
      const cardEl = cardRef.current;
      if (!cardEl) return;

      // 吸附状态下高度变化后重新锚定：
      // - 顶部/底部吸附：隐藏态需按新高度重新算出屏量，只留一条线；
      //   已滑出的底部吸附需贴住工作区底部，避免明细区沉到任务栏后面
      const reAnchorAfterResize = (logicalHeight: number) => {
        const edge = dockEdgeRef.current;
        if (!edge) return;
        const win = getCurrentWindow();
        void Promise.all([getWorkArea(), win.scaleFactor()]).then(
          ([wa, scale]) => {
            if (!wa) return;
            const revealed = revealedPosRef.current;
            if (!revealed) return;
            const peek = Math.round(DOCK_PEEK_MIN * scale);
            const h = Math.round(logicalHeight * scale);
            if (edge === "bottom") {
              const y = dockedPeekedRef.current
                ? wa.y + wa.height - peek
                : wa.y + wa.height - h;
              win
                .setPosition(new PhysicalPosition(revealed.x, y))
                .catch(console.error);
            } else if (edge === "top" && dockedPeekedRef.current) {
              win
                .setPosition(new PhysicalPosition(revealed.x, wa.y - h + peek))
                .catch(console.error);
            }
          },
        );
      };

      if (isExpanding) {
        // 展开动画前先把窗口撑到上次记录的展开高度（兜底 320 已足够容纳明细区），
        // 这样动画全程内容不会被窗口裁切；动画结束后会再精确贴合。
        invoke("resize_floating_usage_window", {
          width: FLOATING_WINDOW_WIDTH,
          height: expandedHeightRef.current,
        }).catch(console.error);
        reAnchorAfterResize(expandedHeightRef.current);
      } else {
        const rect = cardEl.getBoundingClientRect();
        const targetHeight = Math.max(40, Math.ceil(rect.height));
        if (isExpandedRef.current) {
          expandedHeightRef.current = targetHeight;
        }
        invoke("resize_floating_usage_window", {
          width: FLOATING_WINDOW_WIDTH,
          height: targetHeight,
        }).catch(console.error);
        reAnchorAfterResize(targetHeight);
      }
    },
    [getWorkArea],
  );

  // 平滑移动原生窗口到目标位置（物理坐标），rAF 逐帧驱动
  const animateWindowTo = useCallback(
    async (target: { x: number; y: number }, duration = 160) => {
      const win = getCurrentWindow();
      const start = await win.outerPosition();
      const token = ++animTokenRef.current;
      const t0 = performance.now();
      const ease = (t: number) => 1 - Math.pow(1 - t, 3);
      await new Promise<void>((resolve) => {
        const step = () => {
          if (token !== animTokenRef.current) return resolve();
          const p = Math.min(1, (performance.now() - t0) / duration);
          const e = ease(p);
          win
            .setPosition(
              new PhysicalPosition(
                Math.round(start.x + (target.x - start.x) * e),
                Math.round(start.y + (target.y - start.y) * e),
              ),
            )
            .catch(console.error);
          if (p < 1) {
            requestAnimationFrame(step);
          } else {
            resolve();
          }
        };
        requestAnimationFrame(step);
      });
    },
    [],
  );

  // 计算某条边吸附隐藏后窗口应处的物理位置（按吸附深度 peek 留出可见部分）
  const computeHiddenPos = useCallback(
    async (
      edge: DockEdge,
      revealedPos: { x: number; y: number },
      peek: number,
    ) => {
      const win = getCurrentWindow();
      const [wa, sz] = await Promise.all([getWorkArea(), win.outerSize()]);
      if (!wa) return revealedPos;
      switch (edge) {
        case "left":
          return { x: wa.x - sz.width + peek, y: revealedPos.y };
        case "right":
          return { x: wa.x + wa.width - peek, y: revealedPos.y };
        case "top":
          return { x: revealedPos.x, y: wa.y - sz.height + peek };
        case "bottom":
          return { x: revealedPos.x, y: wa.y + wa.height - peek };
      }
    },
    [getWorkArea],
  );

  // 吸附到某条边并按指定深度隐藏（peek 为物理 px，即隐藏后露出的可见量）
  const dockWindow = useCallback(
    async (
      edge: DockEdge,
      revealedPos: { x: number; y: number },
      peek: number,
    ) => {
      const win = getCurrentWindow();
      const [wa, sz] = await Promise.all([getWorkArea(), win.outerSize()]);
      // 滑出位置必须贴合工作区边缘：否则滑出动画会把鼠标"甩"在窗口外，
      // 立刻触发 onMouseLeave → 又自动收回，形成抖动。
      let revealed = revealedPos;
      if (wa) {
        switch (edge) {
          case "left":
            revealed = { x: wa.x, y: revealedPos.y };
            break;
          case "right":
            revealed = { x: wa.x + wa.width - sz.width, y: revealedPos.y };
            break;
          case "top":
            revealed = { x: revealedPos.x, y: wa.y };
            break;
          case "bottom":
            revealed = { x: revealedPos.x, y: wa.y + wa.height - sz.height };
            break;
        }
      }
      dockEdgeRef.current = edge;
      dockedPeekedRef.current = true;
      setDockEdge(edge);
      setDockedPeeked(true);
      peekedAtRef.current = Date.now();
      revealedPosRef.current = revealed;
      saveDockState(edge);
      // 持久化"滑出位置"而非隐藏位置，保证重启后能先回到可见位置再隐藏
      try {
        localStorage.setItem(
          FLOATING_WINDOW_POSITION_KEY,
          JSON.stringify(revealed),
        );
      } catch (e) {
        console.error("Failed to save floating window position", e);
      }
      const hidden = await computeHiddenPos(edge, revealed, peek);
      await animateWindowTo(hidden, 140);
    },
    [animateWindowTo, computeHiddenPos, getWorkArea],
  );

  // 解除吸附（恢复自由移动状态）
  const undockWindow = useCallback(() => {
    dockEdgeRef.current = null;
    dockedPeekedRef.current = false;
    setDockEdge(null);
    setDockedPeeked(false);
    saveDockState(null);
  }, []);

  // 鼠标悬停到藏起的边上时滑出显示
  const revealWindow = useCallback(async () => {
    if (!dockEdgeRef.current || !dockedPeekedRef.current) return;
    dockedPeekedRef.current = false;
    setDockedPeeked(false);
    if (hideTimerRef.current) {
      clearTimeout(hideTimerRef.current);
      hideTimerRef.current = null;
    }
    const pos = revealedPosRef.current;
    if (pos) await animateWindowTo(pos, 160);
  }, [animateWindowTo]);

  // 鼠标移开后延迟收回（保持隐藏态）
  const scheduleHide = useCallback(() => {
    if (!dockEdgeRef.current || dockedPeekedRef.current) return;
    if (hideTimerRef.current) clearTimeout(hideTimerRef.current);
    hideTimerRef.current = setTimeout(async () => {
      hideTimerRef.current = null;
      const edge = dockEdgeRef.current;
      if (!edge || dockedPeekedRef.current) return;
      dockedPeekedRef.current = true;
      setDockedPeeked(true);
      peekedAtRef.current = Date.now();
      const pos = revealedPosRef.current;
      if (pos) {
        const scale = await getCurrentWindow().scaleFactor();
        const minPeek = Math.round(DOCK_PEEK_MIN * scale);
        await animateWindowTo(await computeHiddenPos(edge, pos, minPeek), 160);
      }
    }, DOCK_REVEAL_DELAY);
  }, [animateWindowTo, computeHiddenPos]);

  // 拖拽结束后评估是否贴边：在边缘内则吸附，否则若此前吸附则解除
  const evaluateSnap = useCallback(async () => {
    // 从吸附隐藏态发起的交互：未发生位移=纯点击→滑出；发生位移=拖出→按常规评估
    if (peekedDragRef.current) {
      peekedDragRef.current = false;
      if (!dragMovedRef.current) {
        void revealWindow();
        return;
      }
    }
    if (!dragMovedRef.current) return;
    const win = getCurrentWindow();
    const [pos, wa, sz, scale] = await Promise.all([
      win.outerPosition(),
      getWorkArea(),
      win.outerSize(),
      win.scaleFactor(),
    ]);
    if (!wa) return;
    const threshold = Math.round(SNAP_THRESHOLD * scale);
    const minPeek = Math.round(DOCK_PEEK_MIN * scale);
    // 各边距离：gap>0 表示窗口仍在工作区内、距该边 gap；overhang>0 表示窗口已越过
    // 该边、漏出屏外 overhang（半截漏出也视为已贴边）。
    const edges = [
      { edge: "left" as const, gap: pos.x - wa.x, overhang: wa.x - pos.x },
      {
        edge: "right" as const,
        gap: wa.x + wa.width - (pos.x + sz.width),
        overhang: pos.x + sz.width - (wa.x + wa.width),
      },
      { edge: "top" as const, gap: pos.y - wa.y, overhang: wa.y - pos.y },
      {
        edge: "bottom" as const,
        gap: wa.y + wa.height - (pos.y + sz.height),
        overhang: pos.y + sz.height - (wa.y + wa.height),
      },
    ];
    // 候选边：窗口已越过该边（漏出屏外）或窗口边缘贴近该边（阈值内），
    // 两种情况都吸附，且吸附后统一缩成一条细线。
    const engaged = edges
      .filter((e) => e.overhang > 0 || (e.gap >= 0 && e.gap <= threshold))
      .map((e) => ({
        edge: e.edge,
        overhang: e.overhang,
        dist: e.overhang > 0 ? e.overhang : e.gap,
      }))
      .sort((a, b) => a.dist - b.dist);
    const hit = engaged[0];
    if (!hit) {
      if (dockEdgeRef.current) {
        undockWindow();
        // 已脱离边缘，立即持久化当前真实位置（拖拽过程中因处于吸附态未落盘）
        try {
          localStorage.setItem(
            FLOATING_WINDOW_POSITION_KEY,
            JSON.stringify({ x: pos.x, y: pos.y }),
          );
        } catch (e) {
          console.error("Failed to save floating window position", e);
        }
      }
      return;
    }
    // 吸附深度统一为细线：无论贴边还是半截漏出，隐藏后都只露出一条细线
    const peek = minPeek;
    await dockWindow(hit.edge, { x: pos.x, y: pos.y }, peek);
  }, [dockWindow, getWorkArea, undockWindow, revealWindow]);

  // 轮询判断拖拽是否已结束（最后一次移动距今超过 180ms）
  const startDragSettleCheck = useCallback(() => {
    if (dragSettleTimerRef.current) return;
    dragSettleTimerRef.current = setInterval(() => {
      if (!dragActiveRef.current) {
        if (dragSettleTimerRef.current) {
          clearInterval(dragSettleTimerRef.current);
          dragSettleTimerRef.current = null;
        }
        return;
      }
      if (Date.now() - dragLastMoveAtRef.current > 180) {
        dragActiveRef.current = false;
        if (dragSettleTimerRef.current) {
          clearInterval(dragSettleTimerRef.current);
          dragSettleTimerRef.current = null;
        }
        void evaluateSnap();
      }
    }, 120);
  }, [evaluateSnap]);

  const handleZapDoubleClick = useCallback(async (e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      await invoke("show_main_window");
    } catch (err) {
      console.error("Failed to show main window", err);
    }
  }, []);

  // 鼠标拖拽状态管理
  const [dragStart, setDragStart] = useState<{ x: number; y: number } | null>(
    null,
  );
  const hasDraggedRef = useRef(false);

  const handleMouseDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return; // 仅限左键
    // 吸附隐藏状态：直接进入原生拖拽——可"按住那条线拖出来"；
    // 未移动则视为点击，由 evaluateSnap 判定为"点击滑出"。
    if (dockEdgeRef.current && dockedPeekedRef.current) {
      revealClickGuardRef.current = Date.now();
      peekedDragRef.current = true;
      dragActiveRef.current = true;
      dragMovedRef.current = false;
      dragLastMoveAtRef.current = Date.now();
      startDragSettleCheck();
      getCurrentWindow()
        .startDragging()
        .catch((err) => {
          // 拖拽启动失败（如平台不支持 / 权限缺失）时回退为直接滑出
          peekedDragRef.current = false;
          dragActiveRef.current = false;
          console.error(
            "Failed to start window dragging from docked strip",
            err,
          );
          void revealWindow();
        });
      return;
    }
    dragActiveRef.current = true;
    dragMovedRef.current = false;
    dragLastMoveAtRef.current = Date.now();
    startDragSettleCheck();
    setDragStart({ x: e.screenX, y: e.screenY });
    hasDraggedRef.current = false;
  };

  const handleMouseMove = (e: React.MouseEvent) => {
    if (!dragStart) return;
    // 悬浮窗用 JS startDragging()（显式 IPC），与主窗口 data-tauri-drag-region
    // attribute 驱动不同，Linux/Wayland 下不受 Tauri #13440 的窗口事件异常影响，
    // 因此无需像主窗口那样在 Linux 上禁用拖拽。
    dragLastMoveAtRef.current = Date.now();
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
    // 立即刷新"最后移动时间"，让轮询快速判定拖拽结束
    dragLastMoveAtRef.current = Date.now();
  };

  const handleMouseEnter = () => {
    if (dockEdgeRef.current && dockedPeekedRef.current) {
      void revealWindow();
      revealClickGuardRef.current = Date.now();
    }
  };

  const handleMouseLeave = () => {
    setDragStart(null);
    scheduleHide();
  };

  // 拖拽移动窗口后记录位置，供下次启动恢复；同时维护窗口几何缓存
  useEffect(() => {
    const win = getCurrentWindow();
    let unlistenMoved: (() => void) | undefined;
    let unlistenResized: (() => void) | undefined;
    let saveTimer: ReturnType<typeof setTimeout> | undefined;

    win
      .onMoved(({ payload }) => {
        outerPosRef.current = { x: payload.x, y: payload.y };
        // native 拖拽期间前端收不到 mouse 事件，靠 onMoved 驱动"拖拽结束"判定
        if (dragActiveRef.current) {
          dragLastMoveAtRef.current = Date.now();
          dragMovedRef.current = true;
        }
        // 吸附态下位置由 dockWindow 显式持久化（存滑出位置），这里跳过，
        // 避免把隐藏位置/动画中间位置写入
        if (dockEdgeRef.current) return;
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
        unlistenMoved = off;
      });

    win
      .onResized(({ payload }) => {
        outerSizeRef.current = { width: payload.width, height: payload.height };
      })
      .then((off) => {
        unlistenResized = off;
      });

    // 初始化窗口几何缓存（供吸附隐藏态的光标轮询判定使用）
    win.outerPosition().then((p) => {
      outerPosRef.current = { x: p.x, y: p.y };
    });
    win.outerSize().then((s) => {
      outerSizeRef.current = { width: s.width, height: s.height };
    });

    return () => {
      clearTimeout(saveTimer);
      unlistenMoved?.();
      unlistenResized?.();
    };
  }, []);

  // 吸附隐藏成一条线后：轮询全局光标位置，光标进入窗口外接矩形（含放宽）即滑出。
  // 不依赖 DOM 鼠标事件，避免透明薄边收不到 mouseenter 导致"悬停不滑出"。
  useEffect(() => {
    if (!dockEdge || !dockedPeeked) return;
    const timer = setInterval(async () => {
      // native 拖拽（按住条拖出）期间不触发，避免与拖拽动画互相干扰
      if (dragActiveRef.current) return;
      // 刚隐藏时的宽限期：允许用户把手移开而不触发立即滑出
      if (Date.now() - peekedAtRef.current < DOCK_HOVER_GRACE) return;
      try {
        const pos = await cursorPosition();
        const rp = outerPosRef.current;
        const rs = outerSizeRef.current;
        if (rs.width <= 0 || rs.height <= 0) return;
        const m = DOCK_HOVER_MARGIN;
        if (
          pos.x >= rp.x - m &&
          pos.x <= rp.x + rs.width + m &&
          pos.y >= rp.y - m &&
          pos.y <= rp.y + rs.height + m
        ) {
          if (revealPollRef.current) {
            clearInterval(revealPollRef.current);
            revealPollRef.current = null;
          }
          void revealWindow();
        }
      } catch (e) {
        console.error("Failed to poll cursor position", e);
      }
    }, DOCK_HOVER_POLL_INTERVAL);
    revealPollRef.current = timer;
    return () => {
      if (revealPollRef.current) {
        clearInterval(revealPollRef.current);
        revealPollRef.current = null;
      }
    };
  }, [dockEdge, dockedPeeked, revealWindow]);

  // 卸载时清理拖拽轮询与隐藏定时器
  useEffect(() => {
    return () => {
      if (dragSettleTimerRef.current) {
        clearInterval(dragSettleTimerRef.current);
        dragSettleTimerRef.current = null;
      }
      if (hideTimerRef.current) {
        clearTimeout(hideTimerRef.current);
        hideTimerRef.current = null;
      }
    };
  }, []);

  // 订阅 Tauri IPC 事件以进行数据自动更新
  useUsageEventBridge();
  useUsageCacheBridge();

  const range = useMemo(() => ({ preset: "today" as const }), []);
  const { data, isLoading } = useUsageSummaryByApp(range);
  const { data: modelStatsData, isLoading: isModelStatsLoading } =
    useFloatingModelStats(range);

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

    if (hasShownRef.current) return;
    if (!isLoading && !isModelStatsLoading) {
      invoke<Settings>("get_settings")
        .then(async (settings) => {
          if (settings && settings.enableFloatingUsage) {
            const win = getCurrentWindow();
            // 先恢复上次记录的位置，再显示窗口，避免窗口先出现在默认位置再跳变
            const restored = await restoreWindowPosition(win);
            // 恢复吸附状态：上次贴边隐藏的窗口直接以"一条线"的隐藏态出现
            const savedEdge = restored ? readDockState() : null;
            if (savedEdge) {
              const scale = await win.scaleFactor();
              dockEdgeRef.current = savedEdge;
              dockedPeekedRef.current = true;
              revealedPosRef.current = restored!;
              peekedAtRef.current = Date.now();
              setDockEdge(savedEdge);
              setDockedPeeked(true);
              const hidden = await computeHiddenPos(
                savedEdge,
                restored!,
                Math.round(DOCK_PEEK_MIN * scale),
              );
              await win.setPosition(new PhysicalPosition(hidden.x, hidden.y));
            }
            // 等待浏览器实际绘制完成后再显示窗口，彻底消除白屏闪烁
            requestAnimationFrame(() => {
              win
                .show()
                .then(() => {
                  hasShownRef.current = true;
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
  }, [isLoading, isModelStatsLoading, syncWindowHeight, computeHiddenPos]);

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

  // 累加今日所有应用的 Token 用量与总成本及明细（纯函数，便于单测）
  const summary = useMemo(() => computeFloatingUsageSummary(data), [data]);

  const formattedCost =
    summary.totalCost >= 0.1 || summary.totalCost === 0
      ? fmtUsd(summary.totalCost, 2)
      : fmtUsd(summary.totalCost, 4);

  const modelSummaries = useMemo(() => {
    if (!modelStatsData || modelStatsData.length === 0) return [];
    return modelStatsData
      .map((item) => {
        const cost = parseFloat(item.totalCost) || 0;
        const tokens = item.totalTokens || 0;
        return {
          model: item.model,
          cost,
          tokens,
        };
      })
      .filter((item) => item.tokens > 0 || item.cost > 0)
      .sort((a, b) => b.tokens - a.tokens || b.cost - a.cost);
  }, [modelStatsData]);

  return (
    <div className="w-full h-fit flex flex-col items-center justify-start select-none bg-transparent">
      <div
        ref={cardRef}
        onMouseDown={handleMouseDown}
        onMouseMove={handleMouseMove}
        onMouseUp={handleMouseUp}
        onMouseEnter={handleMouseEnter}
        onMouseLeave={handleMouseLeave}
        className="group relative flex flex-col w-full p-2.5 rounded-xl bg-background/90 dark:bg-background/85 backdrop-blur-md border border-border/50 text-foreground shadow-lg cursor-move"
      >
        {/* 吸附隐藏成一条线时：露出的那条边用一根指示条强调（可见边与吸附边相对） */}
        {dockedPeeked && dockEdge && (
          <div
            className={
              "absolute rounded-full bg-foreground/50 " +
              (dockEdge === "left"
                ? "right-0 top-1/2 -translate-y-1/2 h-8 w-1"
                : dockEdge === "right"
                  ? "left-0 top-1/2 -translate-y-1/2 h-8 w-1"
                  : dockEdge === "top"
                    ? "bottom-0 left-1/2 -translate-x-1/2 w-10 h-1"
                    : "top-0 left-1/2 -translate-x-1/2 w-10 h-1")
            }
          />
        )}
        {/* 关闭悬浮窗按钮：固定悬浮窗右上角（hover 显示），点击置为禁用并销毁窗口，
              设置页开关同步关闭。absolute 脱离顶部栏流，不随展开/收起改变位置 */}
        <button
          title={t("settings.closeFloatingUsage", "Close floating window")}
          onMouseDown={(e) => e.stopPropagation()}
          onClick={(e) => {
            e.stopPropagation();
            void invoke("close_floating_usage_window").catch((err) =>
              console.error("Failed to close floating usage window", err),
            );
          }}
          className="absolute top-1 right-1 z-10 opacity-0 group-hover:opacity-100 transition-all duration-200 p-1 rounded-md hover:bg-red-500/15 hover:text-red-500 text-muted-foreground cursor-pointer"
        >
          <X className="h-3.5 w-3.5" />
        </button>

        {/* 顶部简要信息区域 */}
        <div
          onClick={() => {
            if (hasDraggedRef.current) return;
            // 从隐藏态"按下滑出"时抑制本次点击，避免误触发展开/收起
            if (Date.now() - revealClickGuardRef.current < 300) return;
            setIsExpanded(!isExpanded);
          }}
          className="flex items-center justify-between w-full cursor-pointer pr-6"
        >
          <div className="flex items-center gap-2 min-w-0">
            <div
              onDoubleClick={handleZapDoubleClick}
              onClick={(e) => {
                e.stopPropagation();
              }}
              className="p-1.5 rounded-lg bg-emerald-500/10 hover:bg-emerald-500/20 text-emerald-500 shrink-0 cursor-pointer pointer-events-auto transition-colors active:scale-95 duration-150"
            >
              <Zap className="h-4 w-4" />
            </div>
            <div className="flex flex-col min-w-0 pointer-events-none">
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
        <AnimatePresence
          initial={false}
          onExitComplete={() => {
            setIsFlipped(false);
            setCurrentFace("front");
          }}
        >
          {isExpanded && (
            <motion.div
              key="expanded-content"
              onMouseDown={(e) => e.stopPropagation()}
              onClick={(e) => {
                e.stopPropagation();
                setIsFlipped(!isFlipped);
              }}
              initial={{ opacity: 0, height: 0 }}
              animate={{ opacity: 1, height: "auto" }}
              exit={{ opacity: 0, height: 0 }}
              transition={{ duration: 0.2 }}
              className="overflow-hidden cursor-pointer"
            >
              <div style={{ perspective: 1000 }} className="w-full">
                <motion.div
                  animate={{ rotateY: isFlipped ? 180 : 0 }}
                  transition={{ duration: 0.4, ease: "easeInOut" }}
                  style={{ transformStyle: "preserve-3d" }}
                  className="relative w-full"
                  onAnimationComplete={() => {
                    setCurrentFace(isFlipped ? "back" : "front");
                  }}
                >
                  {/* 正面卡片 (显示输入/输出/Cache等) */}
                  <div
                    style={{
                      backfaceVisibility: "hidden",
                      WebkitBackfaceVisibility: "hidden",
                    }}
                    className={
                      currentFace === "back"
                        ? "relative w-full opacity-0 pointer-events-none"
                        : "relative w-full"
                    }
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
                  </div>

                  {/* 反面卡片 (显示按 Model 分类的汇总消耗) */}
                  <div
                    style={{
                      backfaceVisibility: "hidden",
                      WebkitBackfaceVisibility: "hidden",
                      transform: "rotateY(180deg)",
                      WebkitTransform: "rotateY(180deg)",
                    }}
                    className={
                      currentFace === "front"
                        ? "absolute inset-x-0 top-0 pointer-events-none opacity-0"
                        : "absolute inset-0"
                    }
                  >
                    <div className="pt-2.5 mt-2 border-t border-border/30 flex flex-col text-xs h-full pb-0.5">
                      {/* 模型汇总表头 */}
                      <div className="flex items-center justify-between px-1.5 pb-1.5">
                        <span className="text-[10px] text-muted-foreground font-medium uppercase tracking-wider leading-none">
                          {t("usage.byModel", "By Model")}
                        </span>
                        <span className="text-[10px] text-muted-foreground tabular-nums leading-none">
                          {modelSummaries.length}
                        </span>
                      </div>
                      {/* 模型汇总列表 */}
                      <div
                        onClick={(e) => {
                          const rect = e.currentTarget.getBoundingClientRect();
                          const isScrollbarClick =
                            e.clientX - rect.left > e.currentTarget.clientWidth;
                          if (isScrollbarClick) {
                            e.stopPropagation();
                          }
                        }}
                        className="flex flex-col gap-1 overflow-y-auto pr-0.5 min-h-0 flex-1"
                      >
                        {modelSummaries.length > 0 ? (
                          modelSummaries.map((item) => {
                            const formattedCost =
                              item.cost >= 0.1 || item.cost === 0
                                ? fmtUsd(item.cost, 2)
                                : fmtUsd(item.cost, 4);

                            const tooltipText = `${t("usage.model", "Model")}: ${item.model}\n${t("usage.tokensLabel", "Tokens")}: ${item.tokens.toLocaleString(lang)}\n${t("usage.cost", "Cost")}: ${formattedCost}`;

                            return (
                              <div
                                key={item.model}
                                title={tooltipText}
                                onClick={(e) => {
                                  const rect =
                                    e.currentTarget.getBoundingClientRect();
                                  const isScrollbarClick =
                                    e.clientX - rect.left >
                                    e.currentTarget.clientWidth;
                                  if (isScrollbarClick) {
                                    e.stopPropagation();
                                  }
                                }}
                                className="flex items-center justify-between gap-2 py-1 px-1.5 hover:bg-muted/30 rounded transition-colors min-w-0"
                              >
                                <span className="text-foreground/90 font-medium text-[11px] truncate min-w-0">
                                  {item.model}
                                </span>
                                <span className="font-semibold tabular-nums text-[11px] text-foreground shrink-0 whitespace-nowrap">
                                  {formatTokensShort(item.tokens, lang)}
                                </span>
                              </div>
                            );
                          })
                        ) : (
                          <div className="flex flex-col items-center justify-center py-6 text-muted-foreground text-[10px]">
                            <span>{t("usage.noData", "No Data")}</span>
                          </div>
                        )}
                      </div>
                    </div>
                  </div>
                </motion.div>
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </div>
  );
}
