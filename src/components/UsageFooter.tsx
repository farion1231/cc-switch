import React, { useEffect, useState } from "react";
import { UsageResult } from "../types";
import { AppType } from "../lib/tauri-api";
import { RefreshCw, AlertCircle } from "lucide-react";

interface UsageFooterProps {
  providerId: string;
  appType: AppType;
  isCurrent: boolean;
  usageEnabled: boolean; // 是否启用了用量查询
}

const UsageFooter: React.FC<UsageFooterProps> = ({
  providerId,
  appType,
  isCurrent,
  usageEnabled,
}) => {
  const [usage, setUsage] = useState<UsageResult | null>(null);
  const [loading, setLoading] = useState(false);

  const fetchUsage = async () => {
    setLoading(true);
    try {
      const result = await window.api.queryProviderUsage(
        providerId,
        appType
      );
      setUsage(result);
    } catch (error: any) {
      console.error("查询用量失败:", error);
      setUsage({
        success: false,
        error: error?.message || "查询失败",
      });
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (usageEnabled) {
      fetchUsage();
    }
  }, [providerId, usageEnabled]);

  // 只在启用用量查询且有数据时显示
  if (!usageEnabled || !usage) return null;

  // 错误状态
  if (!usage.success) {
    return (
      <div className="mt-3 pt-3 border-t border-gray-200 dark:border-gray-700">
        <div className="flex items-center justify-between gap-2 text-xs">
          <div className="flex items-center gap-2 text-red-500 dark:text-red-400">
            <AlertCircle size={14} />
            <span>{usage.error || "查询失败"}</span>
          </div>

          {/* 刷新按钮 */}
          <button
            onClick={() => fetchUsage()}
            disabled={loading}
            className="p-1 rounded hover:bg-gray-100 dark:hover:bg-gray-800 transition-colors disabled:opacity-50 flex-shrink-0"
            title="刷新用量"
          >
            <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
          </button>
        </div>
      </div>
    );
  }

  const { planName, expiresAt, isValid, total, used, remaining, unit } =
    usage.data || {};

  // 判断套餐是否失效（isValid 为 false 或未定义时视为有效）
  const isExpired = isValid === false;

  return (
    <div className="mt-3 pt-3 border-t border-gray-200 dark:border-gray-700">
      <div className="flex items-center gap-4 text-xs">
        {/* 左侧：套餐名称 + 过期时间 */}
        <div className="flex items-center gap-2 text-gray-600 dark:text-gray-400 min-w-0 flex-1">
          {planName && (
            <span
              className={`font-medium truncate ${isExpired ? "text-red-500 dark:text-red-400" : ""}`}
              title={planName}
            >
              💰 {planName}
            </span>
          )}
          {expiresAt && (
            <span
              className={`truncate ${isExpired ? "text-red-500 dark:text-red-400" : ""}`}
              title={expiresAt}
            >
              ⏰ {formatDate(expiresAt)}
            </span>
          )}
          {isExpired && (
            <span className="text-red-500 dark:text-red-400 font-medium">
              (已失效)
            </span>
          )}
          {!planName && !expiresAt && <span className="opacity-50">—</span>}
        </div>

        {/* 分隔线 */}
        <div className="h-4 w-px bg-gray-300 dark:bg-gray-600 flex-shrink-0"></div>

        {/* 右侧：额度信息（单行显示，用 | 分隔） */}
        <div className="flex items-center gap-2 text-gray-700 dark:text-gray-300 flex-shrink-0">
          {/* 总额度 */}
          {total !== undefined && (
            <>
              <span className="tabular-nums">
                总: {total === -1 ? "∞" : total.toFixed(2)}
              </span>
              <span className="text-gray-400">|</span>
            </>
          )}

          {/* 已用额度 */}
          {used !== undefined && (
            <>
              <span className="tabular-nums">已用: {used.toFixed(2)}</span>
              <span className="text-gray-400">|</span>
            </>
          )}

          {/* 剩余额度 - 突出显示 */}
          <span className="font-medium text-green-600 dark:text-green-400 tabular-nums">
            剩余: {remaining.toFixed(2)}
          </span>

          <span className="ml-1">{unit}</span>
        </div>

        {/* 刷新按钮 */}
        <button
          onClick={() => fetchUsage()}
          disabled={loading}
          className="p-1 rounded hover:bg-gray-100 dark:hover:bg-gray-800 transition-colors disabled:opacity-50 flex-shrink-0"
          title="刷新用量"
        >
          <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
        </button>
      </div>
    </div>
  );
};

// 日期格式化辅助函数
function formatDate(dateStr: string): string {
  try {
    const date = new Date(dateStr);
    return date.toLocaleDateString("zh-CN", {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
    });
  } catch {
    return dateStr;
  }
}

export default UsageFooter;
