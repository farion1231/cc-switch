import React, { useEffect, useState } from "react";
import { UsageResult, UsageData } from "../types";
import { AppType } from "../lib/tauri-api";
import { RefreshCw, AlertCircle } from "lucide-react";

interface UsageFooterProps {
  providerId: string;
  appType: AppType;
  usageEnabled: boolean; // 是否启用了用量查询
}

const UsageFooter: React.FC<UsageFooterProps> = ({
  providerId,
  appType,
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

  const usageDataList = usage.data || [];

  // 无数据时不显示
  if (usageDataList.length === 0) return null;

  // 根据套餐数量决定布局
  const isSinglePlan = usageDataList.length === 1;

  return (
    <div className="mt-3 pt-3 border-t border-gray-200 dark:border-gray-700">
      {/* 标题行：包含刷新按钮 */}
      <div className="flex items-center justify-between mb-2">
        <span className="text-xs text-gray-500 dark:text-gray-400 font-medium">
          套餐用量
        </span>
        <button
          onClick={() => fetchUsage()}
          disabled={loading}
          className="p-1 rounded hover:bg-gray-100 dark:hover:bg-gray-800 transition-colors disabled:opacity-50"
          title="刷新用量"
        >
          <RefreshCw size={12} className={loading ? "animate-spin" : ""} />
        </button>
      </div>

      {/* 套餐列表 */}
      <div className="flex flex-col gap-3">
        {usageDataList.map((usageData, index) => (
          <UsagePlanItem key={index} data={usageData} />
        ))}
      </div>
    </div>
  );
};

// 单个套餐数据展示组件
const UsagePlanItem: React.FC<{ data: UsageData }> = ({ data }) => {
  const { planName, expiresAt, isValid, total, used, remaining, unit } = data;

  // 判断套餐是否失效（isValid 为 false 或未定义时视为有效）
  const isExpired = isValid === false;

  return (
    <div className="flex items-center justify-between gap-4">
      {/* 左侧：套餐名称 + 过期时间 */}
      <div className="flex items-center gap-2 text-xs text-gray-500 dark:text-gray-400 min-w-0 flex-shrink">
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
          <span className="text-red-500 dark:text-red-400 font-medium text-[10px] px-1.5 py-0.5 bg-red-50 dark:bg-red-900/20 rounded flex-shrink-0">
            已失效
          </span>
        )}
        {!planName && !expiresAt && <span className="opacity-50">—</span>}
      </div>

      {/* 右侧：用量信息（总：xx | 使用：xx | 剩余：xx） */}
      <div className="flex items-center gap-2 text-xs flex-shrink-0">
        {/* 总额度 */}
        {total !== undefined && (
          <>
            <span className="text-gray-500 dark:text-gray-400">总：</span>
            <span className="tabular-nums text-gray-600 dark:text-gray-400">
              {total === -1 ? "∞" : total.toFixed(2)}
            </span>
            <span className="text-gray-400 dark:text-gray-600">|</span>
          </>
        )}

        {/* 已用额度 */}
        {used !== undefined && (
          <>
            <span className="text-gray-500 dark:text-gray-400">使用：</span>
            <span className="tabular-nums text-gray-600 dark:text-gray-400">
              {used.toFixed(2)}
            </span>
            <span className="text-gray-400 dark:text-gray-600">|</span>
          </>
        )}

        {/* 剩余额度 - 突出显示 */}
        <span className="text-gray-500 dark:text-gray-400">剩余：</span>
        <span
          className={`font-semibold tabular-nums ${
            isExpired
              ? "text-red-500 dark:text-red-400"
              : remaining < (total || remaining) * 0.1
                ? "text-orange-500 dark:text-orange-400"
                : "text-green-600 dark:text-green-400"
          }`}
        >
          {remaining.toFixed(2)}
        </span>

        <span className="text-gray-500 dark:text-gray-400">{unit}</span>
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
