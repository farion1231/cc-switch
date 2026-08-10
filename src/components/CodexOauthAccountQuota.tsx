import React from "react";
import { Loader2 } from "lucide-react";
import { useCodexOauthQuotaByAccountId } from "@/lib/query/subscription";
import { SubscriptionQuotaView } from "@/components/SubscriptionQuotaFooter";

interface CodexOauthAccountQuotaProps {
  /** cc-switch 自管的 ChatGPT 账号 ID */
  accountId: string;
}

/**
 * 设置 → 认证中心里，单个 ChatGPT (Codex OAuth) 账号的用量展示。
 *
 * 直接按 accountId 查询 cc-switch 自管 OAuth token 的订阅额度，复用
 * `SubscriptionQuotaView` 的展开布局（进度条 + 重置倒计时 + 刷新按钮），
 * 因此与供应商卡片里的额度展示保持完全一致的观感与状态处理。
 *
 * 面板打开时拉取一次，不轮询；用户可点卡片内的刷新按钮手动更新。
 */
const CodexOauthAccountQuota: React.FC<CodexOauthAccountQuotaProps> = ({
  accountId,
}) => {
  const {
    data: quota,
    isFetching: loading,
    refetch,
  } = useCodexOauthQuotaByAccountId(accountId, {
    enabled: true,
    autoQuery: false,
  });

  // 首次加载占位：账号头部由父组件独立渲染，这里只负责用量区。
  // 用量请求是异步的（Tauri invoke + React Query），加载期间给一个
  // 与最终额度卡片同形状（rounded-xl / border / bg-card）的转圈占位，
  // 这样账号会立刻显示、用量数据到达后原地平滑替换，不产生跳版。
  if (loading && !quota) {
    return (
      <div className="mt-3 flex items-center justify-center rounded-xl border border-border-default bg-card py-5 shadow-sm">
        <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <SubscriptionQuotaView
      quota={quota}
      loading={loading}
      refetch={refetch}
      appIdForExpiredHint="codex_oauth"
      inline={false}
    />
  );
};

export default CodexOauthAccountQuota;
