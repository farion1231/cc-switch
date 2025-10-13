import React, { useEffect, useState, useRef } from "react";
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

  // 记录上次请求的关键参数，防止重复请求
  const lastFetchParamsRef = useRef<string>('');

  const fetchUsage = async () => {
    // 防止并发请求
    if (loading) return;

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
      // 生成当前参数的唯一标识(包含 usageEnabled 状态)
      const currentParams = `${providerId}-${appType}-${usageEnabled}`;

      // 只有参数真正变化时才发起请求
      if (currentParams !== lastFetchParamsRef.current) {
        lastFetchParamsRef.current = currentParams;
        fetchUsage();
      }
    } else {
      // 如果禁用了，清空记录和数据
      lastFetchParamsRef.current = '';
      setUsage(null);
    }
  }, [providerId, usageEnabled, appType]);

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
  const { planName, extra, isValid, invalidMessage, total, used, remaining, unit } = data;

  // 判断套餐是否失效（isValid 为 false 或未定义时视为有效）
  const isExpired = isValid === false;

  return (
    <div className="flex items-center gap-3">
      {/* 标题部分：25% */}
      <div className="text-xs text-gray-500 dark:text-gray-400 min-w-0" style={{ width: "25%" }}>
        {planName ? (
          <span
            className={`font-medium truncate block ${isExpired ? "text-red-500 dark:text-red-400" : ""}`}
            title={planName}
          >
            💰 {planName}
          </span>
        ) : (
          <span className="opacity-50">—</span>
        )}
      </div>

      {/* 扩展字段：30% */}
      <div className="text-xs text-gray-500 dark:text-gray-400 min-w-0 flex items-center gap-2" style={{ width: "30%" }}>
        {extra && (
          <span
            className={`truncate ${isExpired ? "text-red-500 dark:text-red-400" : ""}`}
            title={extra}
          >
            {extra}
          </span>
        )}
        {isExpired && (
          <span className="text-red-500 dark:text-red-400 font-medium text-[10px] px-1.5 py-0.5 bg-red-50 dark:bg-red-900/20 rounded flex-shrink-0">
            {invalidMessage || "已失效"}
          </span>
        )}
      </div>

      {/* 用量信息：45% */}
      <div className="flex items-center justify-end gap-2 text-xs flex-shrink-0" style={{ width: "45%" }}>
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
        {remaining !== undefined && (
          <>
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
          </>
        )}

        {unit && <span className="text-gray-500 dark:text-gray-400">{unit}</span>}
      </div>
    </div>
  );
};


export default UsageFooter;
