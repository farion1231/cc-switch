import { useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { getAppLabel } from "@/config/appConfig";
import type { ProxyStack } from "@/types/proxy";

/** 发布的 Stack 模型 id（去重、排序：换顺序不影响能选哪些模型）。 */
function publishedSignature(stack: ProxyStack): string {
  const ids = new Set(stack.members.flatMap((member) => member.modelIds));
  return [...ids].sort().join("\n");
}

/**
 * Claude Code 只在启动时向代理取一次模型列表：Stack 模式下发布的模型变了（编辑、删除名单里的
 * 供应商，或者别处的改动），提示重启 Claude Code。加入、移出名单时保存的提示已经说过要重启，
 * 调用方在发起之前调返回的函数，跳过紧接着的那一次重查。Codex 另有按进程判断的横幅
 * （`CodexStaleClientsNotice`），这里不管。
 *
 * 按 `updatedAt` 比较：内容没变的重查也会消费「跳过」，不会留着吞掉之后真正的变化。
 */
export function useStackModelsChangedHint(
  appId: string,
  stack: ProxyStack | undefined,
  updatedAt: number,
): () => void {
  const { t } = useTranslation();
  const baseline = useRef<string | null>(null);
  const skipNext = useRef(false);
  const signature =
    appId === "claude" && stack?.active ? publishedSignature(stack) : null;

  useEffect(() => {
    if (signature === null) {
      baseline.current = null;
      skipNext.current = false;
      return;
    }
    const previous = baseline.current;
    baseline.current = signature;
    if (skipNext.current) {
      skipNext.current = false;
      return;
    }
    if (previous !== null && previous !== signature) {
      toast.info(
        t("provider.stackModelsChanged", { client: getAppLabel(appId) }),
        { closeButton: true },
      );
    }
    // `t` 不在依赖里：切换语言不是一次重查，不能消费「跳过」。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [appId, signature, updatedAt]);

  return useCallback(() => {
    skipNext.current = true;
  }, []);
}
