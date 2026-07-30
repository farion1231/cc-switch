import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from "react";
import { useQueryClient } from "@tanstack/react-query";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { remoteApi } from "@/lib/api/remote";
import {
  createRuntimeTransition,
  getRuntimeSnapshot,
  setRuntimeSnapshot,
  subscribeRuntime,
} from "@/lib/runtime/store";
import type { RemoteTargetConfig, RuntimeSnapshot } from "@/lib/runtime/types";
import { extractErrorMessage } from "@/utils/errorUtils";

/**
 * 目标切换只清理带主机语义的 Provider/Usage 缓存；Settings 等桌面级缓存不应被无关切换抹掉。
 */
function clearEnvironmentQueryCaches(
  queryClient: ReturnType<typeof useQueryClient>,
): void {
  queryClient.removeQueries({ queryKey: ["providers"] });
  queryClient.removeQueries({ queryKey: ["usage"] });
}

interface RuntimeTargetContextValue {
  snapshot: RuntimeSnapshot;
  targets: RemoteTargetConfig[];
  refreshTargets: () => Promise<void>;
  upsertTarget: (target: RemoteTargetConfig) => Promise<void>;
  deleteTarget: (targetId: string) => Promise<void>;
  setActiveTarget: (targetId?: string) => Promise<void>;
}

const RuntimeTargetContext = createContext<RuntimeTargetContextValue | null>(
  null,
);

export function RuntimeTargetProvider({ children }: { children: ReactNode }) {
  const queryClient = useQueryClient();
  const snapshot = useSyncExternalStore(
    subscribeRuntime,
    getRuntimeSnapshot,
    getRuntimeSnapshot,
  );
  const [targets, setTargets] = useState<RemoteTargetConfig[]>([]);

  const refreshTargets = useCallback(async () => {
    setTargets(await remoteApi.listTargets());
  }, []);

  useEffect(() => {
    let active = true;
    let unlisten: UnlistenFn | undefined;
    void Promise.all([remoteApi.getSnapshot(), remoteApi.listTargets()])
      .then(([nextSnapshot, nextTargets]) => {
        if (!active) return;
        setRuntimeSnapshot(nextSnapshot);
        setTargets(nextTargets);
      })
      .catch((error) => {
        console.error("[RuntimeTarget] 初始化远程运行时失败", error);
      });
    void remoteApi
      .onStatus((nextSnapshot) => {
        if (!active) return;
        // 后端主动状态变化也先停止旧 generation；取消完成后才发布新快照，
        // 避免旧请求在新目标已经可见时继续更新组件状态。
        void queryClient.cancelQueries().then(() => {
          if (!active) return;
          clearEnvironmentQueryCaches(queryClient);
          setRuntimeSnapshot(nextSnapshot);
        });
      })
      .then((off) => {
        if (active) unlisten = off;
        else off();
      });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [queryClient]);

  const setActiveTarget = useCallback(
    async (targetId?: string) => {
      const previous = getRuntimeSnapshot();
      // 先等待取消完成再发布 connecting，否则旧 queryFn 可能读取新 runtime 快照，
      // 形成“旧 key、却访问新主机”的混合请求。
      await queryClient.cancelQueries();
      setRuntimeSnapshot(createRuntimeTransition(previous, targetId));
      try {
        const next = await remoteApi.setActiveTarget(targetId);
        clearEnvironmentQueryCaches(queryClient);
        setRuntimeSnapshot(next);
      } catch (error) {
        const fallback = await remoteApi.getSnapshot().catch(() => ({
          status: "offline" as const,
          generation: previous.generation + 1,
          activeTargetId: targetId,
          errorCode: "REMOTE_CONNECTION_ERROR",
          errorMessage: extractErrorMessage(error),
        }));
        clearEnvironmentQueryCaches(queryClient);
        setRuntimeSnapshot(fallback);
        toast.error(extractErrorMessage(error));
      }
    },
    [queryClient],
  );

  const upsertTarget = useCallback(
    async (target: RemoteTargetConfig) => {
      await remoteApi.upsertTarget(target);
      await refreshTargets();
    },
    [refreshTargets],
  );

  const deleteTarget = useCallback(
    async (targetId: string) => {
      await remoteApi.deleteTarget(targetId);
      await refreshTargets();
      setRuntimeSnapshot(await remoteApi.getSnapshot());
    },
    [refreshTargets],
  );

  const value = useMemo(
    () => ({
      snapshot,
      targets,
      refreshTargets,
      upsertTarget,
      deleteTarget,
      setActiveTarget,
    }),
    [
      snapshot,
      targets,
      refreshTargets,
      upsertTarget,
      deleteTarget,
      setActiveTarget,
    ],
  );

  return (
    <RuntimeTargetContext.Provider value={value}>
      {children}
    </RuntimeTargetContext.Provider>
  );
}

export function useRuntimeTarget(): RuntimeTargetContextValue {
  const value = useContext(RuntimeTargetContext);
  if (!value) {
    throw new Error("useRuntimeTarget 必须在 RuntimeTargetProvider 内使用");
  }
  return value;
}
