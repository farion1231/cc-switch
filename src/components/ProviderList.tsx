import React, { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Provider, ProviderTestResult } from "../types";
import {
  Play,
  Edit3,
  Trash2,
  CheckCircle2,
  Users,
  Check,
  Loader2,
  TestTube2,
  CircleAlert,
  CircleCheck,
  Zap,
  Square,
  CheckSquare,
  MoreHorizontal,
  Bug,
} from "lucide-react";
import { buttonStyles, cardStyles, badgeStyles, cn } from "../lib/styles";
import { AppType } from "../lib/tauri-api";

const CACHE_DURATION_MS = 30_000;

type TestStatus = "idle" | "loading" | "success" | "error";

interface ProviderConnectionState {
  status: TestStatus;
  message?: string;
  detail?: string;
  statusCode?: number;
  latencyMs?: number;
  testedAt?: number;
}

// 不再在列表中显示分类徽章，避免造成困惑

interface ProviderListProps {
  appType: AppType;
  providers: Record<string, Provider>;
  currentProviderId: string;
  onSwitch: (id: string) => void;
  onDelete: (id: string) => void;
  onEdit: (id: string) => void;
  onNotify?: (
    message: string,
    type: "success" | "error",
    duration?: number,
  ) => void;
}

const ProviderList: React.FC<ProviderListProps> = ({
  appType,
  providers,
  currentProviderId,
  onSwitch,
  onDelete,
  onEdit,
  onNotify,
}) => {
  const { t, i18n } = useTranslation();
  const [testStates, setTestStates] = useState<
    Record<string, ProviderConnectionState>
  >({});
  const [isTestingAll, setIsTestingAll] = useState(false);
  const [selectedProviders, setSelectedProviders] = useState<Set<string>>(new Set());
  const [isBatchMode, setIsBatchMode] = useState(false);
  const [showDiagnostics, setShowDiagnostics] = useState<Record<string, boolean>>({});

  const summarizeResultDetail = (
    result: ProviderTestResult,
  ): string | undefined => {
    if (result.success) {
      return undefined;
    }

    const detail = result.detail?.trim();
    if (detail) {
      // 检查是否为HTML响应（如403错误页面）
      if (detail.startsWith("<!DOCTYPE") || detail.startsWith("<html")) {
        // 尝试从HTML中提取title或错误信息
        const titleMatch = detail.match(/<title[^>]*>([^<]+)<\/title>/i);
        if (titleMatch && titleMatch[1]) {
          const title = titleMatch[1].trim();
          if (title.includes("403") || title.includes("Forbidden")) {
            return "访问被拒绝 (403 Forbidden) - 服务器拒绝了请求";
          }
          return title;
        }

        // 检查常见的HTTP错误状态码
        if (detail.includes("403") || detail.includes("Forbidden")) {
          return "访问被拒绝 (403 Forbidden) - 服务器拒绝了请求";
        }
        if (detail.includes("401") || detail.includes("Unauthorized")) {
          return "身份验证失败 (401 Unauthorized) - API密钥无效或缺失";
        }
        if (detail.includes("404") || detail.includes("Not Found")) {
          return "端点不存在 (404 Not Found) - API地址不正确";
        }
        if (detail.includes("429") || detail.includes("Too Many Requests")) {
          return "请求频率限制 (429) - 请求过于频繁，请稍后重试";
        }
        if (detail.includes("500") || detail.includes("Internal Server Error")) {
          return "服务器内部错误 (500) - 服务端出现问题";
        }

        return "服务器返回HTML错误页面";
      }

      try {
        const parsed = JSON.parse(detail);
        if (typeof parsed === "string") {
          return parsed.trim() || undefined;
        }
        if (parsed && typeof parsed === "object") {
          // 处理 {"Error": "upstream_error", "details": "..."} 格式
          if (typeof parsed.Error === "string") {
            let errorMsg = parsed.Error.trim();
            // 处理嵌套的 details 字段
            if (typeof parsed.details === "string") {
              try {
                const nestedDetails = JSON.parse(parsed.details);
                if (nestedDetails && typeof nestedDetails === "object") {
                  if (typeof nestedDetails.detail === "string") {
                    errorMsg = `${errorMsg}: ${nestedDetails.detail.trim()}`;
                  } else if (typeof nestedDetails.title === "string") {
                    errorMsg = `${errorMsg}: ${nestedDetails.title.trim()}`;
                  }
                }
              } catch {
                // 如果解析失败，直接使用原始字符串
                errorMsg = `${errorMsg}: ${parsed.details.trim()}`;
              }
            }
            return errorMsg || undefined;
          }
          // 标准错误格式
          if (typeof parsed.error === "string") {
            return parsed.error.trim() || undefined;
          }
          if (
            parsed.error &&
            typeof parsed.error === "object" &&
            typeof parsed.error.message === "string"
          ) {
            return parsed.error.message.trim() || undefined;
          }
          if (typeof parsed.message === "string") {
            return parsed.message.trim() || undefined;
          }
          if (typeof parsed.title === "string") {
            return parsed.title.trim() || undefined;
          }
        }
      } catch {
        // detail is not JSON; use raw string below
      }
      return detail;
    }

    // 如果没有detail，根据status码提供具体的错误信息
    if (result.status) {
      const statusCode = result.status;
      switch (statusCode) {
        case 403:
          return "访问被拒绝 (403 Forbidden) - 服务器拒绝了请求，可能需要检查API密钥或网络访问权限";
        case 401:
          return "身份验证失败 (401 Unauthorized) - API密钥无效或缺失";
        case 404:
          return "端点不存在 (404 Not Found) - API地址不正确";
        case 429:
          return "请求频率限制 (429) - 请求过于频繁，请稍后重试";
        case 500:
          return "服务器内部错误 (500) - 服务端出现问题";
        case 502:
          return "网关错误 (502) - 服务器网关问题";
        case 503:
          return "服务不可用 (503) - 服务器暂时不可用";
        default:
          if (statusCode >= 400 && statusCode < 500) {
            return `客户端错误 (${statusCode}) - 请求问题`;
          }
          if (statusCode >= 500) {
            return `服务器错误 (${statusCode}) - 服务端问题`;
          }
      }
    }

    const message = result.message?.trim();
    return message || undefined;
  };

  const truncate = (value: string, max = 140) => {
    if (value.length <= max) {
      return value;
    }
    return `${value.slice(0, max)}…`;
  };

  // 提供连接诊断信息的辅助函数
  const getDiagnosticInfo = (provider: Provider, testState?: ProviderConnectionState) => {
    const diagnostics: string[] = [];

    // 检查API密钥配置
    if (appType === "claude") {
      const apiKey = provider.settingsConfig?.env?.ANTHROPIC_AUTH_TOKEN;
      if (!apiKey || typeof apiKey !== "string" || apiKey.trim() === "") {
        diagnostics.push("❌ API密钥缺失或为空");
      } else if (apiKey.length < 10) {
        diagnostics.push("⚠️ API密钥长度可能不足");
      } else {
        diagnostics.push("✅ API密钥已配置");
      }
    } else if (appType === "codex") {
      const apiKey = provider.settingsConfig?.auth?.OPENAI_API_KEY;
      if (!apiKey || typeof apiKey !== "string" || apiKey.trim() === "") {
        diagnostics.push("❌ API密钥缺失或为空");
      } else if (apiKey.length < 10) {
        diagnostics.push("⚠️ API密钥长度可能不足");
      } else {
        diagnostics.push("✅ API密钥已配置");
      }
    }

    // 检查API地址
    const apiUrl = getApiUrl(provider);
    if (apiUrl === t("provider.notConfigured")) {
      diagnostics.push("❌ API地址未配置");
    } else if (apiUrl === t("provider.configError")) {
      diagnostics.push("❌ 配置解析错误");
    } else {
      diagnostics.push(`✅ API地址: ${apiUrl}`);

      // 检查URL格式
      try {
        const url = new URL(apiUrl);
        if (!url.protocol.startsWith("http")) {
          diagnostics.push("⚠️ URL协议不正确");
        }
        if (url.hostname.includes("localhost") || url.hostname.includes("127.0.0.1")) {
          diagnostics.push("ℹ️ 使用本地地址，请确保服务正在运行");
        }
      } catch {
        diagnostics.push("⚠️ API地址格式可能不正确");
      }
    }

    // 添加测试状态相关信息
    if (testState) {
      if (testState.statusCode === 403) {
        diagnostics.push("🔍 403错误可能原因: API密钥无效、账户被限制、需要特殊权限");
      } else if (testState.statusCode === 401) {
        diagnostics.push("🔍 401错误可能原因: API密钥过期或格式错误");
      } else if (testState.statusCode === 404) {
        diagnostics.push("🔍 404错误可能原因: API地址错误或服务不可用");
      } else if (testState.statusCode && testState.statusCode >= 500) {
        diagnostics.push("🔍 服务器错误可能原因: 服务临时不可用或维护中");
      }
    }

    return diagnostics;
  };

  const isCacheFresh = (state?: ProviderConnectionState) => {
    if (!state?.testedAt) return false;
    return Date.now() - state.testedAt < CACHE_DURATION_MS;
  };
  // 提取API地址（兼容不同供应商配置：Claude env / Codex TOML）
  const getApiUrl = (provider: Provider): string => {
    try {
      const cfg = provider.settingsConfig;
      // Claude/Anthropic: 从 env 中读取
      if (cfg?.env?.ANTHROPIC_BASE_URL) {
        return cfg.env.ANTHROPIC_BASE_URL;
      }
      // Codex: 从 TOML 配置中解析 base_url
      if (typeof cfg?.config === "string" && cfg.config.includes("base_url")) {
        // 支持单/双引号
        const match = cfg.config.match(/base_url\s*=\s*(['"])([^'\"]+)\1/);
        if (match && match[2]) return match[2];
      }
      return t("provider.notConfigured");
    } catch {
      return t("provider.configError");
    }
  };

  const handleUrlClick = async (url: string) => {
    try {
      await window.api.openExternal(url);
    } catch (error) {
      console.error(t("console.openLinkFailed"), error);
      onNotify?.(
        `${t("console.openLinkFailed")}: ${String(error)}`,
        "error",
        4000,
      );
    }
  };

  const handleTestClick = async (providerId: string, force = false) => {
    const existing = testStates[providerId];
    if (!force && isCacheFresh(existing) && existing?.status !== "loading") {
      const type = existing.status === "success" ? "success" : "error";
      onNotify?.(
        t("providerTest.cachedNotice"),
        type,
        type === "success" ? 2000 : 3000,
      );
      return;
    }

    setTestStates((prev) => ({
      ...prev,
      [providerId]: {
        ...prev[providerId],
        status: "loading",
        testedAt: Date.now(),
      },
    }));

    try {
      const result = await window.api.testProviderConnection(providerId, appType);
      const detail = summarizeResultDetail(result);
      const testedAt = Date.now();

      setTestStates((prev) => ({
        ...prev,
        [providerId]: {
          status: result.success ? "success" : "error",
          message: result.message,
          detail,
          statusCode: result.status,
          latencyMs: result.latencyMs,
          testedAt,
        },
      }));

      if (!result.success) {
        onNotify?.(
          t("providerTest.notifyError", {
            error: detail ?? t("providerTest.unknownError"),
          }),
          "error",
          5000,
        );
      }
    } catch (error) {
      console.error(t("console.testProviderFailed"), error);
      const fallback =
        error instanceof Error ? error.message : String(error ?? "");
      setTestStates((prev) => ({
        ...prev,
        [providerId]: {
          status: "error",
          message: fallback,
          detail: fallback,
          testedAt: Date.now(),
        },
      }));
      onNotify?.(
        t("providerTest.notifyError", { error: fallback }),
        "error",
        5000,
      );
    }
  };

  const handleTestAll = async () => {
    if (Object.keys(providers).length === 0) {
      onNotify?.(t("provider.noProviders"), "error", 3000);
      return;
    }

    setIsTestingAll(true);

    let successCount = 0;
    let errorCount = 0;
    const providerIds = Object.keys(providers);

    try {
      // 逐个测试供应商，每完成一个就立即显示结果
      for (const providerId of providerIds) {
        // 设置当前供应商为测试中状态
        setTestStates((prev) => ({
          ...prev,
          [providerId]: {
            status: "loading",
            testedAt: Date.now(),
          },
        }));

        try {
          // 测试单个供应商
          const result = await window.api.testProviderConnection(providerId, appType);
          const detail = summarizeResultDetail(result);
          const testedAt = Date.now();

          // 立即更新该供应商的测试结果
          setTestStates((prev) => ({
            ...prev,
            [providerId]: {
              status: result.success ? "success" : "error",
              message: result.message,
              detail,
              statusCode: result.status,
              latencyMs: result.latencyMs,
              testedAt,
            },
          }));

          // 统计成功/失败数量
          if (result.success) {
            successCount++;
          } else {
            errorCount++;
          }
        } catch (error) {
          console.error(t("console.testProviderFailed"), providerId, error);
          const fallback =
            error instanceof Error ? error.message : String(error ?? "");

          // 立即更新该供应商的错误状态
          setTestStates((prev) => ({
            ...prev,
            [providerId]: {
              status: "error",
              message: fallback,
              detail: fallback,
              testedAt: Date.now(),
            },
          }));

          errorCount++;
        }
      }

      // 所有测试完成后显示汇总通知
      if (errorCount === 0) {
        onNotify?.(
          t("providerTest.allSuccess", { count: successCount }),
          "success",
          4000,
        );
      } else if (successCount === 0) {
        onNotify?.(
          t("providerTest.allError", { count: errorCount }),
          "error",
          5000,
        );
      } else {
        onNotify?.(
          t("providerTest.partialSuccess", {
            success: successCount,
            error: errorCount,
          }),
          "error",
          5000,
        );
      }
    } catch (error) {
      console.error(t("console.testAllProvidersFailed"), error);
      const fallback =
        error instanceof Error ? error.message : String(error ?? "");

      onNotify?.(
        t("providerTest.notifyError", { error: fallback }),
        "error",
        5000,
      );
    } finally {
      setIsTestingAll(false);
    }
  };

  // 批量操作相关函数
  const toggleBatchMode = () => {
    setIsBatchMode(!isBatchMode);
    setSelectedProviders(new Set());
  };

  const toggleProviderSelection = (providerId: string) => {
    setSelectedProviders(prev => {
      const newSet = new Set(prev);
      if (newSet.has(providerId)) {
        newSet.delete(providerId);
      } else {
        newSet.add(providerId);
      }
      return newSet;
    });
  };

  const toggleSelectAll = () => {
    if (selectedProviders.size === sortedProviders.length) {
      setSelectedProviders(new Set());
    } else {
      setSelectedProviders(new Set(sortedProviders.map(p => p.id)));
    }
  };

  const handleBatchDelete = async () => {
    if (selectedProviders.size === 0) {
      onNotify?.(t("provider.batchDelete.noSelection"), "error", 3000);
      return;
    }

    // 检查是否包含当前供应商
    const selectedArray = Array.from(selectedProviders);
    const currentProviderIndex = selectedArray.indexOf(currentProviderId);
    if (currentProviderIndex !== -1) {
      onNotify?.(t("provider.batchDelete.cannotDeleteCurrent"), "error", 3000);
      return;
    }

    const confirmed = window.confirm(
      t("provider.batchDelete.confirm", { count: selectedProviders.size })
    );
    if (!confirmed) return;

    let successCount = 0;
    let errorCount = 0;

    try {
      for (const providerId of selectedArray) {
        try {
          await onDelete(providerId);
          successCount++;
        } catch (error) {
          console.error(`删除供应商失败: ${providerId}`, error);
          errorCount++;
        }
      }

      // 清空选择
      setSelectedProviders(new Set());
      setIsBatchMode(false);

      // 显示结果通知
      if (errorCount === 0) {
        onNotify?.(
          t("provider.batchDelete.success", { count: successCount }),
          "success",
          4000
        );
      } else {
        onNotify?.(
          t("provider.batchDelete.partialSuccess", {
            success: successCount,
            error: errorCount,
          }),
          "error",
          5000
        );
      }
    } catch (error) {
      console.error("批量删除失败", error);
      onNotify?.(t("provider.batchDelete.failed"), "error", 5000);
    }
  };

  const renderStatusRow = (
    providerId: string,
    override?: ProviderConnectionState,
  ) => {
    const state = override ?? testStates[providerId];
    if (!state || state.status === "idle") {
      return null;
    }

    if (state.status === "loading") {
      return (
        <div className="mt-2 flex items-center gap-1.5 text-xs text-amber-600 dark:text-amber-400">
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
          <span>{t("providerTest.testing")}</span>
        </div>
      );
    }

    if (state.status === "success") {
      const summary: string[] = [t("providerTest.success")];
      if (typeof state.latencyMs === "number") {
        summary.push(
          t("providerTest.latency", {
            latency: Math.round(state.latencyMs),
          }),
        );
      }
      if (typeof state.statusCode === "number") {
        summary.push(
          t("providerTest.status", { status: state.statusCode }),
        );
      }

      return (
        <div className="mt-2 flex items-center gap-1.5 text-xs text-emerald-600 dark:text-emerald-400">
          <CircleCheck className="h-3.5 w-3.5" />
          <span>{summary.join(" · ")}</span>
        </div>
      );
    }

    // 直接显示错误详情，状态码会单独显示在徽章中
    const detail = state.detail ?? state.message ?? t("providerTest.unknownError");
    const errorMessage = truncate(detail, 100);

    return (
      <div className="mt-2 space-y-2">
        <div className="flex items-start gap-1.5 text-xs text-red-600 dark:text-red-400">
          <CircleAlert className="h-3.5 w-3.5 mt-0.5 flex-shrink-0" />
          <span className="max-w-[28rem] break-words">
            {typeof state.statusCode === "number" && (
              <span className="inline-block bg-red-100 dark:bg-red-900/30 text-red-700 dark:text-red-300 px-1.5 py-0.5 rounded font-mono text-xs font-medium mr-1.5">
                {state.statusCode}
              </span>
            )}
            <span>{t("providerTest.error", { message: errorMessage })}</span>
          </span>
        </div>
      </div>
    );
  };

  // 列表页不再提供 Claude 插件按钮，统一在“设置”中控制

  // 对供应商列表进行排序
  const sortedProviders = useMemo(() => {
    return Object.values(providers).sort((a, b) => {
      const timeA = a.createdAt || 0;
      const timeB = b.createdAt || 0;

      if (timeA === 0 && timeB === 0) {
        const locale = i18n.language === "zh" ? "zh-CN" : "en-US";
        return a.name.localeCompare(b.name, locale);
      }

      if (timeA === 0) return -1;
      if (timeB === 0) return 1;

      return timeA - timeB;
    });
  }, [providers, i18n.language]);

  return (
    <div className="space-y-4">
      {sortedProviders.length === 0 ? (
        <div className="text-center py-12">
          <div className="w-16 h-16 mx-auto mb-4 bg-gray-100 rounded-full flex items-center justify-center">
            <Users size={24} className="text-gray-400" />
          </div>
          <h3 className="text-lg font-medium text-gray-900 dark:text-gray-100 mb-2">
            {t("provider.noProviders")}
          </h3>
          <p className="text-gray-500 dark:text-gray-400 text-sm">
            {t("provider.noProvidersDescription")}
          </p>
        </div>
      ) : (
        <>
          {/* 批量操作按钮区域 */}
          <div className="flex items-center justify-between bg-gray-50 dark:bg-gray-800 rounded-lg p-4">
            <div className="flex items-center gap-4">
              <div className="text-sm text-gray-600 dark:text-gray-300">
                {t("provider.totalCount", { count: sortedProviders.length })}
              </div>

              {isBatchMode && (
                <div className="flex items-center gap-2">
                  <button
                    onClick={toggleSelectAll}
                    className="inline-flex items-center gap-2 px-3 py-1.5 text-sm font-medium rounded-md transition-colors bg-gray-200 dark:bg-gray-700 text-gray-700 dark:text-gray-300 hover:bg-gray-300 dark:hover:bg-gray-600"
                  >
                    {selectedProviders.size === sortedProviders.length ? (
                      <CheckSquare className="h-4 w-4" />
                    ) : (
                      <Square className="h-4 w-4" />
                    )}
                    {selectedProviders.size === sortedProviders.length
                      ? t("provider.batch.deselectAll")
                      : t("provider.batch.selectAll")}
                  </button>

                  {selectedProviders.size > 0 && (
                    <span className="text-sm text-gray-600 dark:text-gray-400">
                      {t("provider.batch.selected", { count: selectedProviders.size })}
                    </span>
                  )}
                </div>
              )}
            </div>

            <div className="flex items-center gap-2">
              {isBatchMode && selectedProviders.size > 0 && (
                <button
                  onClick={handleBatchDelete}
                  className={cn(
                    "inline-flex items-center gap-2 px-4 py-2 text-sm font-medium rounded-md transition-colors bg-red-500 text-white hover:bg-red-600 dark:bg-red-600 dark:hover:bg-red-700"
                  )}
                >
                  <Trash2 className="h-4 w-4" />
                  {t("provider.batchDelete.button")}
                </button>
              )}

              <button
                onClick={toggleBatchMode}
                className={cn(
                  "inline-flex items-center gap-2 px-4 py-2 text-sm font-medium rounded-md transition-colors",
                  isBatchMode
                    ? "bg-gray-300 text-gray-700 hover:bg-gray-400 dark:bg-gray-600 dark:text-gray-300 dark:hover:bg-gray-500"
                    : "bg-blue-500 text-white hover:bg-blue-600 dark:bg-blue-600 dark:hover:bg-blue-700"
                )}
              >
                <MoreHorizontal className="h-4 w-4" />
                {isBatchMode ? t("provider.batch.exit") : t("provider.batch.mode")}
              </button>

              {!isBatchMode && (
                <button
                  onClick={handleTestAll}
                  disabled={isTestingAll}
                  className={cn(
                    "inline-flex items-center gap-2 px-4 py-2 text-sm font-medium rounded-md transition-colors",
                    isTestingAll
                      ? "bg-gray-300 text-gray-500 cursor-not-allowed dark:bg-gray-600 dark:text-gray-400"
                      : "bg-emerald-500 text-white hover:bg-emerald-600 dark:bg-emerald-600 dark:hover:bg-emerald-700"
                  )}
                >
                  {isTestingAll ? (
                    <>
                      <Loader2 className="h-4 w-4 animate-spin" />
                      {t("providerTest.testingAll")}
                    </>
                  ) : (
                    <>
                      <Zap className="h-4 w-4" />
                      {t("providerTest.testAll")}
                    </>
                  )}
                </button>
              )}
            </div>
          </div>

          <div className="space-y-3">
            {sortedProviders.map((provider) => {
            const isCurrent = provider.id === currentProviderId;
            const apiUrl = getApiUrl(provider);
            const testState = testStates[provider.id];

            return (
              <div
                key={provider.id}
                className={cn(
                  isCurrent ? cardStyles.selected : cardStyles.interactive,
                  isBatchMode && "border-l-4 border-l-blue-500 dark:border-l-blue-400",
                )}
              >
                <div className="flex items-start justify-between">
                  {/* 批量模式复选框 */}
                  {isBatchMode && (
                    <div className="mr-3 pt-1">
                      <button
                        onClick={() => toggleProviderSelection(provider.id)}
                        disabled={isCurrent}
                        className={cn(
                          "p-1 rounded transition-colors",
                          selectedProviders.has(provider.id)
                            ? "text-blue-600 dark:text-blue-400"
                            : "text-gray-400 dark:text-gray-500 hover:text-gray-600 dark:hover:text-gray-300",
                          isCurrent && "cursor-not-allowed opacity-50"
                        )}
                        title={isCurrent ? t("provider.batch.cannotSelectCurrent") : t("provider.batch.select")}
                      >
                        {selectedProviders.has(provider.id) ? (
                          <CheckSquare className="h-5 w-5" />
                        ) : (
                          <Square className="h-5 w-5" />
                        )}
                      </button>
                    </div>
                  )}

                  <div className="flex-1">
                    <div className="flex items-center gap-3 mb-2">
                      <h3 className="font-medium text-gray-900 dark:text-gray-100">
                        {provider.name}
                      </h3>
                      {/* 分类徽章已移除 */}
                      <div
                        className={cn(
                          badgeStyles.success,
                          !isCurrent && "invisible",
                        )}
                      >
                        <CheckCircle2 size={12} />
                        {t("provider.currentlyUsing")}
                      </div>
                    </div>

                    <div className="flex items-center gap-2 text-sm">
                      {provider.websiteUrl ? (
                        <button
                          onClick={(e) => {
                            e.preventDefault();
                            handleUrlClick(provider.websiteUrl!);
                          }}
                          className="inline-flex items-center gap-1 text-blue-500 dark:text-blue-400 hover:opacity-90 transition-colors"
                          title={t("providerForm.visitWebsite", {
                            url: provider.websiteUrl,
                          })}
                        >
                          {provider.websiteUrl}
                        </button>
                      ) : (
                        <span
                          className="text-gray-500 dark:text-gray-400"
                          title={apiUrl}
                        >
                          {apiUrl}
                        </span>
                      )}
                    </div>

                    {renderStatusRow(provider.id, testState)}

                    {/* 诊断信息 */}
                    {showDiagnostics[provider.id] && (
                      <div className="mt-3 p-3 bg-gray-50 dark:bg-gray-800 rounded-md border border-gray-200 dark:border-gray-700">
                        <h4 className="text-xs font-medium text-gray-700 dark:text-gray-300 mb-2 flex items-center gap-1">
                          <Bug className="h-3 w-3" />
                          连接诊断信息
                        </h4>
                        <div className="space-y-1">
                          {getDiagnosticInfo(provider, testState).map((diagnostic, index) => (
                            <div key={index} className="text-xs text-gray-600 dark:text-gray-400 font-mono">
                              {diagnostic}
                            </div>
                          ))}
                        </div>
                        <div className="mt-2 pt-2 border-t border-gray-200 dark:border-gray-700">
                          <div className="text-xs text-gray-500 dark:text-gray-500">
                            💡 提示：如果遇到403/401错误，请检查API密钥是否正确且有效
                          </div>
                        </div>
                      </div>
                    )}
                  </div>

                  {!isBatchMode && (
                  <div className="flex items-center gap-2 ml-4">
                    <button
                      onClick={(event) =>
                        handleTestClick(provider.id, event.shiftKey)
                      }
                      disabled={testState?.status === "loading"}
                      title={t("providerTest.tooltip")}
                      className={cn(
                        "inline-flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-md transition-colors bg-emerald-500 text-white hover:bg-emerald-600 dark:bg-emerald-600 dark:hover:bg-emerald-700",
                        testState?.status === "loading" &&
                          "cursor-wait opacity-80",
                      )}
                    >
                      {testState?.status === "loading" ? (
                        <Loader2 className="h-4 w-4 animate-spin" />
                      ) : (
                        <TestTube2 className="h-4 w-4" />
                      )}
                      {t("providerTest.button")}
                    </button>

                    <button
                      onClick={() => onSwitch(provider.id)}
                      disabled={isCurrent}
                      className={cn(
                        "inline-flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-md transition-colors w-[90px] justify-center whitespace-nowrap",
                        isCurrent
                          ? "bg-gray-100 text-gray-400 dark:bg-gray-800 dark:text-gray-500 cursor-not-allowed"
                          : "bg-blue-500 text-white hover:bg-blue-600 dark:bg-blue-600 dark:hover:bg-blue-700",
                      )}
                    >
                      {isCurrent ? <Check size={14} /> : <Play size={14} />}
                      {isCurrent ? t("provider.inUse") : t("provider.enable")}
                    </button>

                    <button
                      onClick={() => onEdit(provider.id)}
                      className={buttonStyles.icon}
                      title={t("provider.editProvider")}
                    >
                      <Edit3 size={16} />
                    </button>

                    <button
                      onClick={() => {
                        setShowDiagnostics(prev => ({
                          ...prev,
                          [provider.id]: !prev[provider.id]
                        }));
                      }}
                      className={cn(
                        buttonStyles.icon,
                        showDiagnostics[provider.id]
                          ? "text-blue-500 bg-blue-100 dark:bg-blue-500/20"
                          : "text-gray-500 hover:text-blue-500 hover:bg-blue-100 dark:text-gray-400 dark:hover:text-blue-400 dark:hover:bg-blue-500/10"
                      )}
                      title="显示诊断信息"
                    >
                      <Bug size={16} />
                    </button>

                    <button
                      onClick={() => onDelete(provider.id)}
                      disabled={isCurrent}
                      className={cn(
                        buttonStyles.icon,
                        isCurrent
                          ? "text-gray-400 cursor-not-allowed"
                          : "text-gray-500 hover:text-red-500 hover:bg-red-100 dark:text-gray-400 dark:hover:text-red-400 dark:hover:bg-red-500/10",
                      )}
                      title={t("provider.deleteProvider")}
                    >
                      <Trash2 size={16} />
                    </button>
                  </div>
                )}
                </div>
              </div>
            );
          })}
          </div>
        </>
      )}
    </div>
  );
};

export default ProviderList;
