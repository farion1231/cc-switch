import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import {
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import { arrayMove, sortableKeyboardCoordinates } from "@dnd-kit/sortable";

import { terminalApi } from "@/lib/api/terminal";
import { terminalQueue } from "@/lib/terminalQueue";
import { extractErrorMessage } from "@/utils/errorUtils";
import type {
  CreateTerminalPayload,
  TerminalHubState,
  TerminalInstance,
} from "@/types/terminal";

const EMPTY_STATE: TerminalHubState = {
  instances: [],
  projectDirs: [],
  expanded: false,
};

/** 终端会话存活状态（桌面原生终端进程：running / stopped）。 */
export type TerminalAliveStatus = "running" | "idle";

/** 存活探测间隔 */
const ALIVE_POLL_MS = 3000;
const MAX_PROJECT_DIRS = 20;

/** 把目录提到历史最前（去重 + 截断）。 */
function rememberDir(dirs: string[], dir: string): string[] {
  const trimmed = dir.trim();
  if (!trimmed) return dirs;
  return [trimmed, ...dirs.filter((item) => item !== trimmed)].slice(
    0,
    MAX_PROJECT_DIRS,
  );
}

/**
 * 终端控制台的状态管理：持久化状态 + 进程存活轮询 + 打开/停止/增删改。
 *
 * 持久化落在 Rust 侧 `~/.cc-switch/settings.json`（`terminalHub` 字段），
 * 打开终端时拉起系统原生终端并记录可管理 PID。
 */
export function useTerminalHub() {
  const { t } = useTranslation();
  const [state, setState] = useState<TerminalHubState>(EMPTY_STATE);
  const [loaded, setLoaded] = useState(false);
  const [availableTerminals, setAvailableTerminals] = useState<string[]>([]);
  /** instanceId → 存活状态（running / idle；缺失 = 已停止） */
  const [alive, setAlive] = useState<Record<string, TerminalAliveStatus>>({});
  /** 正在执行打开/停止操作的实例 id */
  const [busyId, setBusyId] = useState<string | null>(null);
  /** 工作台当前展示的实例 id */
  const [activeInstanceId, setActiveInstanceId] = useState<string | null>(null);
  /** instanceId → 内嵌终端 pty id（面板内 PTY 会话） */
  const [embeddedPty, setEmbeddedPty] = useState<Record<string, number>>({});
  /** instanceId → 内嵌终端是否已退出 */
  const [embeddedExit, setEmbeddedExit] = useState<Record<string, boolean>>({});
  /** instanceId → 重置计数（递增触发内嵌会话重启） */
  const [resetNonce, setResetNonce] = useState<Record<string, number>>({});

  // 拖拽排序传感器：8px 激活距离避免与点击/右键冲突
  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: { distance: 8 },
    }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );

  // 轮询与异步回调都要读到最新状态，用 ref 避免闭包读到旧值
  const stateRef = useRef(state);
  useEffect(() => {
    stateRef.current = state;
  }, [state]);

  const embeddedPtyRef = useRef(embeddedPty);
  useEffect(() => {
    embeddedPtyRef.current = embeddedPty;
  }, [embeddedPty]);

  const applyState = useCallback((next: TerminalHubState) => {
    stateRef.current = next;
    setState(next);
  }, []);

  const persist = useCallback(
    async (next: TerminalHubState) => {
      applyState(next);
      try {
        await terminalApi.saveState(next);
      } catch (error) {
        console.error("[terminalHub] failed to persist state", error);
      }
    },
    [applyState],
  );

  // 初次加载：状态 + 可用终端列表。
  useEffect(() => {
    let cancelled = false;
    let retryTimer: number | undefined;
    let attempt = 0;
    const MAX_RETRIES = 5;
    const RETRY_DELAY_MS = 2500;

    const bootstrap = async () => {
      try {
        const [hub, terminals] = await Promise.all([
          terminalApi.getState(),
          terminalApi.detectAvailable(),
        ]);
        if (cancelled) return;
        attempt = 0;
        applyState({
          ...EMPTY_STATE,
          ...hub,
          instances: hub.instances ?? [],
          projectDirs: hub.projectDirs ?? [],
        });
        setAvailableTerminals(terminals);
      } catch (error) {
        console.error("[terminalHub] failed to load state", error);
        if (cancelled) return;
        attempt += 1;
        if (attempt <= MAX_RETRIES) {
          retryTimer = window.setTimeout(
            () => void bootstrap(),
            RETRY_DELAY_MS,
          );
        } else {
          toast.error(
            t("terminalHub.loadFailed", {
              defaultValue: "终端会话加载失败：{{error}}",
              error: extractErrorMessage(error),
            }),
          );
        }
      } finally {
        if (!cancelled) setLoaded(true);
      }
    };

    void bootstrap();
    return () => {
      cancelled = true;
      if (retryTimer !== undefined) window.clearTimeout(retryTimer);
    };
  }, [applyState, t]);

  // 窗口聚焦时重新拉取后端状态：HUD 与大屏是两个独立前端实例，各自只在
  // 挂载时 bootstrap 一次。在 HUD 中新建/删除终端后切回大屏（或反之），
  // 另一窗口的本地 state 仍是旧快照，导致两侧终端列表数量不一致。
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    const setup = async () => {
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        unlisten = await getCurrentWindow().onFocusChanged(({ payload }) => {
          if (!payload) return;
          void (async () => {
            try {
              const hub = await terminalApi.getState();
              applyState({
                ...stateRef.current,
                ...hub,
                instances: hub.instances ?? [],
                projectDirs: hub.projectDirs ?? [],
              });
            } catch (error) {
              console.error("[terminalHub] failed to reload on focus", error);
            }
          })();
        });
      } catch (error) {
        console.error("[terminalHub] failed to watch window focus", error);
      }
    };
    void setup();
    return () => {
      unlisten?.();
    };
  }, [applyState]);

  // 存活探测：只对有 PID 的实例轮询，避免无意义的 IPC
  const instancesRef = useRef(state.instances);
  useEffect(() => {
    instancesRef.current = state.instances;
  }, [state.instances]);

  useEffect(() => {
    if (!loaded) return;

    const tick = async () => {
      const targets = instancesRef.current.filter(
        (instance) => typeof instance.pid === "number",
      );
      if (targets.length === 0) return;

      const results = await Promise.all(
        targets.map(async (instance) => {
          try {
            return [
              instance.id,
              await terminalApi.checkAlive(instance.pid!),
            ] as const;
          } catch {
            return [instance.id, false] as const;
          }
        }),
      );

      setAlive((prev) => {
        const next = { ...prev };
        for (const [id, isAlive] of results) {
          if (isAlive) next[id] = "running";
          else delete next[id];
        }
        return next;
      });
    };

    void tick();
    const timer = window.setInterval(() => void tick(), ALIVE_POLL_MS);
    return () => window.clearInterval(timer);
  }, [loaded]);

  /** 注册/注销内嵌终端会话（面板内 PTY）。 */
  const registerEmbedded = useCallback((id: string, ptyId: number | null) => {
    setEmbeddedPty((prev) => {
      const next = { ...prev };
      if (ptyId === null) delete next[id];
      else next[id] = ptyId;
      return next;
    });
  }, []);

  const setEmbeddedExited = useCallback((id: string, exited: boolean) => {
    setEmbeddedExit((prev) => {
      const next = { ...prev };
      if (!exited) delete next[id];
      else next[id] = true;
      return next;
    });
  }, []);

  /** 递增重置计数：通知内嵌终端组件重启会话（重置/恢复历史会话用）。 */
  const bumpReset = useCallback((id: string) => {
    setResetNonce((prev) => ({ ...prev, [id]: (prev[id] ?? 0) + 1 }));
  }, []);

  /** 实例当前是否有活跃会话（系统终端 pid 存活或内嵌 pty 未退出）。 */
  const instanceActive = useCallback(
    (id: string): boolean => {
      const instance = stateRef.current.instances.find(
        (item) => item.id === id,
      );
      if (!instance) return false;
      const embeddedOk =
        typeof embeddedPtyRef.current[id] === "number" && !embeddedExit[id];
      if (embeddedOk) return true;
      return typeof instance.pid === "number" && alive[id] === "running";
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [alive, embeddedExit],
  );

  const setExpanded = useCallback(
    (expanded: boolean) => {
      void persist({ ...stateRef.current, expanded });
    },
    [persist],
  );

  /** 选择/新增项目目录（同时记住为「上次选择」） */
  const selectProjectDir = useCallback(
    (dir: string) => {
      const trimmed = dir.trim();
      if (!trimmed) return;
      const current = stateRef.current;
      void persist({
        ...current,
        projectDirs: rememberDir(current.projectDirs, trimmed),
        lastProjectDir: trimmed,
      });
    },
    [persist],
  );

  const removeProjectDir = useCallback(
    (dir: string) => {
      const current = stateRef.current;
      void persist({
        ...current,
        projectDirs: current.projectDirs.filter((item) => item !== dir),
        lastProjectDir:
          current.lastProjectDir === dir ? undefined : current.lastProjectDir,
      });
    },
    [persist],
  );

  /**
   * 新建终端。返回新建实例的 id（失败返回 null）——调用方据此立即选中
   * 新终端，避免「建完了还得再点一下」。
   */
  const createTerminal = useCallback(
    async (payload: CreateTerminalPayload): Promise<string | null> => {
      const before = new Set(stateRef.current.instances.map((item) => item.id));
      try {
        const next = await terminalApi.createInstance(payload);
        applyState(next);
        const created = next.instances.find((item) => !before.has(item.id));
        return created?.id ?? null;
      } catch (error) {
        console.error("[terminalHub] failed to create terminal", error);
        toast.error(
          `${t("terminalHub.createFailed", { defaultValue: "新建终端失败" })}: ${extractErrorMessage(error)}`,
        );
        return null;
      }
    },
    [applyState, t],
  );

  const deleteTerminal = useCallback(
    async (id: string) => {
      // 先关掉内嵌会话（如有）
      const ptyId = embeddedPtyRef.current[id];
      if (ptyId !== undefined) {
        try {
          await terminalApi.closeEmbedded(ptyId);
        } catch {
          // 忽略
        }
        registerEmbedded(id, null);
        setEmbeddedExited(id, false);
      }
      try {
        const next = await terminalApi.deleteInstance(id);
        applyState(next);
        setAlive((prev) => {
          const copy = { ...prev };
          delete copy[id];
          return copy;
        });
        // 连同该终端的发送队列一起清理（队列按终端隔离，不留孤儿数据）
        terminalQueue.dispose(id);
      } catch (error) {
        console.error("[terminalHub] failed to delete terminal", error);
        toast.error(
          `${t("terminalHub.deleteFailed", { defaultValue: "删除终端失败" })}: ${extractErrorMessage(error)}`,
        );
      }
    },
    [applyState, registerEmbedded, setEmbeddedExited, t],
  );

  /** 打开终端：拉起系统原生终端并记录可管理 PID。 */
  const openTerminal = useCallback(
    async (id: string) => {
      const instance = stateRef.current.instances.find(
        (item) => item.id === id,
      );
      if (!instance) return;

      setBusyId(id);
      try {
        const pid = await terminalApi.launch({
          instanceId: instance.id,
          app: instance.app,
          projectDir: instance.projectDir,
          tool: instance.tool,
          customCommand: instance.customCommand ?? null,
          args: instance.args ?? null,
          terminal: instance.terminal ?? null,
          providerId: null,
        });

        const current = stateRef.current;
        const next: TerminalHubState = {
          ...current,
          instances: current.instances.map((item) =>
            item.id === id
              ? {
                  ...item,
                  pid,
                  lastLaunchAt: Math.floor(Date.now() / 1000),
                }
              : item,
          ),
          projectDirs: rememberDir(current.projectDirs, instance.projectDir),
          lastProjectDir: instance.projectDir,
        };
        await persist(next);
        // pid <= 0 表示不可管理（如 Windows Terminal）：不标记运行，保持「未启动」状态
        if (pid > 0) {
          setAlive((prev) => ({ ...prev, [id]: "running" }));
        }
        toast.success(t("terminalHub.opened", { defaultValue: "终端已打开" }));
      } catch (error) {
        console.error("[terminalHub] failed to launch terminal", error);
        toast.error(
          `${t("terminalHub.openFailed", { defaultValue: "打开终端失败" })}: ${extractErrorMessage(error)}`,
        );
      } finally {
        setBusyId(null);
      }
    },
    [persist, t],
  );

  /** 聚焦已运行的终端窗口；窗口已关闭等无法聚焦时自动重新拉起。 */
  const focusTerminal = useCallback(
    async (id: string) => {
      const instance = stateRef.current.instances.find(
        (item) => item.id === id,
      );
      if (!instance) return;
      if (typeof instance.pid !== "number") {
        await openTerminal(id);
        return;
      }
      try {
        const focused = await terminalApi.focus(instance.pid);
        if (!focused) await openTerminal(id);
      } catch {
        await openTerminal(id);
      }
    },
    [openTerminal],
  );

  /** 终止终端：优先关闭内嵌 PTY 会话；否则终止系统终端进程树。 */
  const stopTerminal = useCallback(
    async (id: string) => {
      const ptyId = embeddedPtyRef.current[id];
      if (ptyId !== undefined) {
        try {
          await terminalApi.closeEmbedded(ptyId);
          registerEmbedded(id, null);
          setEmbeddedExited(id, true);
          toast.success(
            t("terminalHub.stopped", { defaultValue: "终端已停止" }),
          );
          return;
        } catch (error) {
          console.error("[terminalHub] failed to close embedded", error);
          toast.error(
            `${t("terminalHub.stopFailed", { defaultValue: "停止终端失败" })}: ${extractErrorMessage(error)}`,
          );
          return;
        }
      }

      const instance = stateRef.current.instances.find(
        (item) => item.id === id,
      );
      if (!instance) return;

      if (typeof instance.pid !== "number") {
        toast.error(
          t("terminalHub.stopUnavailable", {
            defaultValue: "该终端不由当前会话管理，无法停止",
          }),
        );
        return;
      }

      setBusyId(id);
      try {
        await terminalApi.kill(instance.pid);
        setAlive((prev) => {
          const copy = { ...prev };
          delete copy[id];
          return copy;
        });
        toast.success(t("terminalHub.stopped", { defaultValue: "终端已停止" }));
      } catch (error) {
        console.error("[terminalHub] failed to kill terminal", error);
        toast.error(
          `${t("terminalHub.stopFailed", { defaultValue: "停止终端失败" })}: ${extractErrorMessage(error)}`,
        );
      } finally {
        setBusyId(null);
      }
    },
    [registerEmbedded, setEmbeddedExited, t],
  );

  /**
   * 清除并重新初始化终端：结束旧进程（内嵌 PTY 或系统终端）、删掉落盘历史，
   * 再重新拉起——否则重启应用后仍会回放旧内容并自动恢复旧会话。
   */
  const resetTerminal = useCallback(
    async (id: string) => {
      // 清落盘历史：让本次「重置」真正得到全新会话（重启后不再恢复）
      try {
        await terminalApi.clearTerminalHistory(id);
      } catch {
        // 历史文件可能不存在，忽略
      }
      // 「重置」语义是全新会话：同时清掉显式指定的恢复目标，
      // 否则重启应用后又会跳回同一个会话。
      {
        const current = stateRef.current;
        const target = current.instances.find((item) => item.id === id);
        if (target?.lastSessionId) {
          await persist({
            ...current,
            instances: current.instances.map((item) =>
              item.id === id ? { ...item, lastSessionId: undefined } : item,
            ),
          });
        }
      }
      const ptyId = embeddedPtyRef.current[id];
      if (ptyId !== undefined) {
        try {
          await terminalApi.closeEmbedded(ptyId);
        } catch {
          // 旧会话可能已退出
        }
        registerEmbedded(id, null);
        setEmbeddedExited(id, false);
        // 通知内嵌组件用最新配置（含恢复的会话参数）重启
        bumpReset(id);
        toast.success(
          t("terminalHub.resetDone", {
            defaultValue: "终端已重新初始化",
          }),
        );
        return;
      }

      const instance = stateRef.current.instances.find(
        (item) => item.id === id,
      );
      if (!instance) return;

      if (typeof instance.pid === "number") {
        try {
          await terminalApi.kill(instance.pid);
        } catch {
          // 旧进程可能已退出
        }
        const current = stateRef.current;
        void persist({
          ...current,
          instances: current.instances.map((item) =>
            item.id === id ? { ...item, pid: undefined } : item,
          ),
        });
        setAlive((prev) => {
          const copy = { ...prev };
          delete copy[id];
          return copy;
        });
      }
      // 无内嵌会话：不再拉起系统原生终端窗口（避免重置时弹出无关窗口）。
      // 已挂载的面板会话随 bumpReset 重新连接；未挂载的实例下次选中时自动新建。
      bumpReset(id);
      toast.success(
        t("terminalHub.resetDone", {
          defaultValue: "终端已重新初始化",
        }),
      );
    },
    [bumpReset, persist, registerEmbedded, setEmbeddedExited, t],
  );

  /** 局部更新某个终端实例的字段（名称 / 附加参数等），并持久化。 */
  const updateInstance = useCallback(
    (id: string, patch: Partial<TerminalInstance>) => {
      const current = stateRef.current;
      void persist({
        ...current,
        instances: current.instances.map((item) =>
          item.id === id ? { ...item, ...patch } : item,
        ),
      });
    },
    [persist],
  );

  /** 拖拽排序终端实例（dnd-kit arrayMove 语义），并持久化。 */
  const reorderInstances = useCallback(
    (activeId: string, overId: string) => {
      if (!activeId || activeId === overId) return;
      const current = stateRef.current;
      const ids = current.instances.map((item) => item.id);
      const oldIndex = ids.indexOf(activeId);
      const newIndex = ids.indexOf(overId);
      if (oldIndex === -1 || newIndex === -1) return;
      void persist({
        ...current,
        instances: arrayMove(current.instances, oldIndex, newIndex),
      });
    },
    [persist],
  );

  return {
    loaded,
    state,
    alive,
    busyId,
    availableTerminals,
    sensors,
    workspaceOpen: false,
    activeInstanceId,
    setWorkspaceOpen: (_open: boolean) => undefined,
    setActiveInstanceId,
    setExpanded,
    selectProjectDir,
    removeProjectDir,
    createTerminal,
    deleteTerminal,
    openTerminal,
    focusTerminal,
    stopTerminal,
    resetTerminal,
    updateInstance,
    reorderInstances,
    embeddedPty,
    embeddedExit,
    resetNonce,
    registerEmbedded,
    setEmbeddedExited,
    bumpReset,
    instanceActive,
  };
}
