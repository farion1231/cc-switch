import { useQueryClient } from "@tanstack/react-query";
import type { AppId } from "@/lib/api/types";
import { useTauriEvent } from "./useTauriEvent";

interface KeyRateLimitedPayload {
  appType: AppId;
  providerId: string;
  keyId: string;
  /** LimitSignal reason — "429" / "quota_message" / "user_regex" / "5xx"。 */
  reason: string;
  /** key 冷却到的时间（unix seconds）。 */
  cooldownUntil: number;
}

/**
 * 后端 KeyRing 在某把 key 命中「真配额窗口」（cooldown ≥ 5 分钟）时
 * 发射 `key-rate-limited` 事件。本 hook 收到后 invalidate 该 provider 的
 * apiKeys 查询 —— 触发 `useApiKeys` refetch，进而把 row 上每把 key 的
 * `cooldownUntil` / `failure_count` 拉到 UI，5h 配额进度条立刻反映出
 * 这把 key 已经不可用。
 *
 * 短冷却（30s / 60s）的事件不广播 —— 见 forwarder.rs 的
 * QUOTA_REFRESH_THRESHOLD_SECS。这避免每次 30s 试探失败都触发一轮网络刷新。
 *
 * 与 useUsageCacheBridge 互补：后者同步「成功拉到的用量快照」，前者
 * 同步「key 进入冷却」这一状态翻转。
 */
export function useKeyRateLimitedBridge() {
  const queryClient = useQueryClient();

  useTauriEvent<KeyRateLimitedPayload>("key-rate-limited", (payload) => {
    queryClient.invalidateQueries({
      queryKey: ["apiKeys", payload.providerId, payload.appType],
    });
    // 同时把每把 key 自己的 usage 查询也标 stale —— cooldown 翻转后
    // progress bar 的 reset 倒计时也依赖 usage 数据，refetch 才能拿到
    // 最新的 reset_at / used%。
    queryClient.invalidateQueries({
      queryKey: ["keyUsage", payload.keyId, payload.appType],
    });
  });
}