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
  Send,
  ChevronDown,
  ChevronUp,
  Copy,
  CheckCheck,
  ArrowUpDown,
  Clock,
  Search,
  X,
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
  const [collapsedDiagnostics, setCollapsedDiagnostics] = useState<Record<string, boolean>>({});
  const [showFullError, setShowFullError] = useState<Record<string, boolean>>({});
  const [testMessage, setTestMessage] = useState<Record<string, string>>({});
  const [testResults, setTestResults] = useState<Record<string, { loading: boolean; response?: string }>>({});
  const [copiedResponse, setCopiedResponse] = useState<Record<string, boolean>>({});
  // 排序模式: 'default' | 'latency' | 'error'
  const [sortMode, setSortMode] = useState<'default' | 'latency' | 'error'>('default');
  // 搜索关键词
  const [searchKeyword, setSearchKeyword] = useState('');

  const summarizeResultDetail = (
    result: ProviderTestResult,
  ): string | undefined => {
    if (result.success) {
      return undefined;
    }

    const detail = result.detail?.trim();
    if (detail) {
      // 如果是HTML响应，提取title或提示是HTML页面
      if (detail.startsWith("<!DOCTYPE") || detail.startsWith("<html")) {
        const titleMatch = detail.match(/<title[^>]*>([^<]+)<\/title>/i);
        if (titleMatch && titleMatch[1]) {
          return titleMatch[1].trim();
        }
        return "[服务器返回HTML页面，非JSON响应]";
      }

      // 尝试格式化JSON，让其更易读
      try {
        const parsed = JSON.parse(detail);
        // 返回格式化后的JSON字符串，保留原始结构
        return JSON.stringify(parsed, null, 2);
      } catch {
        // 如果不是JSON，直接返回原始字符串
        return detail;
      }
    }

    // 如果没有detail，返回message
    const message = result.message?.trim();
    return message || "未知错误";
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

    const providerIds = Object.keys(providers);

    // 先将所有供应商设置为测试中状态
    setTestStates((prev) => {
      const newStates = { ...prev };
      providerIds.forEach((providerId) => {
        newStates[providerId] = {
          status: "loading",
          testedAt: Date.now(),
        };
      });
      return newStates;
    });

    try {
      // 并发测试所有供应商
      const testPromises = providerIds.map(async (providerId) => {
        try {
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

          return { success: result.success, providerId };
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

          return { success: false, providerId };
        }
      });

      // 等待所有测试完成
      const results = await Promise.all(testPromises);

      // 统计成功/失败数量
      const successCount = results.filter(r => r.success).length;
      const errorCount = results.filter(r => !r.success).length;

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

      // 如果有成功的测试结果,自动启用按延迟排序
      if (successCount > 0) {
        setSortMode('latency');
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

  const handleSendTestMessage = async (providerId: string) => {
    const message = testMessage[providerId] || "Hello";
    if (!message.trim()) {
      onNotify?.("请输入测试消息", "error", 2000);
      return;
    }

    setTestResults(prev => ({
      ...prev,
      [providerId]: { loading: true }
    }));

    try {
      const response = await window.api.sendTestMessage(providerId, message, appType);

      // 尝试格式化JSON响应以便更好地显示
      let formattedResponse: string;
      try {
        const parsed = JSON.parse(response);
        formattedResponse = JSON.stringify(parsed, null, 2);
      } catch {
        // 如果不是JSON，直接显示原始响应
        formattedResponse = response;
      }

      setTestResults(prev => ({
        ...prev,
        [providerId]: { loading: false, response: formattedResponse }
      }));
    } catch (error) {
      const errorMsg = error instanceof Error ? error.message : String(error);
      setTestResults(prev => ({
        ...prev,
        [providerId]: {
          loading: false,
          response: `错误: ${errorMsg}`
        }
      }));
    }
  };

  const handleCopyResponse = async (providerId: string) => {
    const response = testResults[providerId]?.response;
    if (!response) return;

    try {
      await navigator.clipboard.writeText(response);
      setCopiedResponse(prev => ({ ...prev, [providerId]: true }));
      onNotify?.("响应已复制到剪贴板", "success", 2000);
      
      // 2秒后重置复制状态
      setTimeout(() => {
        setCopiedResponse(prev => ({ ...prev, [providerId]: false }));
      }, 2000);
    } catch (error) {
      console.error("复制失败:", error);
      onNotify?.("复制失败", "error", 2000);
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
    const isExpanded = showFullError[providerId];

    return (
      <div className="mt-2 space-y-2 w-full">
        <div className="flex items-start justify-between gap-3 text-xs text-red-600 dark:text-red-400">
          {/* 左侧：图标 + 状态码 + 错误消息 */}
          <div className="flex items-start gap-1.5 flex-1 min-w-0">
            <CircleAlert className="h-3.5 w-3.5 mt-0.5 flex-shrink-0" />
            <div className="flex items-center gap-2 flex-wrap">
              {typeof state.statusCode === "number" && (
                <span className="inline-block bg-red-100 dark:bg-red-900/30 text-red-700 dark:text-red-300 px-1.5 py-0.5 rounded font-mono text-xs font-medium">
                  {state.statusCode}
                </span>
              )}
              <span className="text-xs break-words">
                {state.message ?? "所有测试端点和认证方式都无法访问"}
              </span>
            </div>
          </div>
          
          {/* 右侧：折叠按钮 */}
          <div className="flex items-center gap-1.5 flex-shrink-0">
            <button
              onClick={() => setShowFullError(prev => ({ ...prev, [providerId]: !prev[providerId] }))}
              className="text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200 transition-colors p-1 rounded hover:bg-gray-100 dark:hover:bg-gray-700"
              title={isExpanded ? "隐藏测试" : "隐藏测试"}
            >
              {isExpanded ? (
                <ChevronUp className="h-4 w-4" />
              ) : (
                <ChevronDown className="h-4 w-4" />
              )}
            </button>
          </div>
        </div>
        
        {/* 展开时显示左右两列布局 */}
        {isExpanded && (
          <div className="mt-3 w-full border-amber-300 border rounded-lg p-4">
            <div className="grid gap-4 lg:grid-cols-2 w-full">
              {/* 左侧：完整错误信息 */}
              <div className="flex flex-col p-4 bg-red-50 dark:bg-red-900/10 rounded-lg border border-red-200 dark:border-red-800">
                <h4 className="text-sm font-semibold text-gray-700 dark:text-gray-300 mb-3">
                  所有测试端点和认证方式都无法访问
                </h4>
                <div className="flex-1 overflow-y-auto text-sm text-gray-700 dark:text-gray-300 break-words whitespace-pre-wrap font-mono max-h-[400px] leading-relaxed p-3 bg-white/50 dark:bg-gray-900/30 rounded">
                  {detail}
                </div>
              </div>

              {/* 右侧：测试输入区域 */}
              <div className="flex flex-col p-4 bg-gray-50 dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700">
                <h4 className="text-sm font-semibold text-gray-700 dark:text-gray-300 mb-4">
                  发送测试消息
                </h4>
                <div className="flex gap-2 mb-4">
                  <input
                    type="text"
                    value={testMessage[providerId] || ""}
                    onChange={(e) => setTestMessage(prev => ({ ...prev, [providerId]: e.target.value }))}
                    placeholder="输入测试消息 (如: Hello)"
                    className="flex-1 px-3 py-2.5 text-sm border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-green-500 focus:border-transparent transition-all duration-200"
                    onKeyPress={(e) => {
                      if (e.key === 'Enter') {
                        handleSendTestMessage(providerId);
                      }
                    }}
                  />
                  <button
                    onClick={() => handleSendTestMessage(providerId)}
                    disabled={testResults[providerId]?.loading}
                    className={cn(
                      "inline-flex items-center justify-center gap-1.5 px-4 py-2.5 text-sm font-medium rounded-lg transition-all duration-200 whitespace-nowrap min-w-[80px]",
                      testResults[providerId]?.loading
                        ? "bg-gray-300 text-gray-500 cursor-not-allowed dark:bg-gray-600 dark:text-gray-400"
                        : "bg-green-500 text-white hover:bg-green-600 active:bg-green-700 hover:shadow-md dark:bg-green-600 dark:hover:bg-green-700"
                    )}
                  >
                    {testResults[providerId]?.loading ? (
                      <>
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                        <span>发送中</span>
                      </>
                    ) : (
                      <>
                        <Send className="h-3.5 w-3.5" />
                        <span>发送</span>
                      </>
                    )}
                  </button>
                </div>

                {/* 显示测试响应 */}
                {testResults[providerId]?.response && (
                  <div className="flex-1 flex flex-col p-4 bg-white dark:bg-gray-900 rounded-lg border border-gray-300 dark:border-gray-600 shadow-sm">
                    <div className="flex items-center justify-between mb-3">
                      <h5 className="text-sm font-semibold text-gray-700 dark:text-gray-300">响应消息</h5>
                      <div className="flex items-center gap-2">
                        <span className="text-xs text-gray-500 dark:text-gray-400">
                          {new Date().toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })}
                        </span>
                        <button
                          onClick={() => handleCopyResponse(providerId)}
                          className="flex items-center gap-1 px-2 py-1 text-xs font-medium text-gray-600 dark:text-gray-400 hover:text-blue-600 dark:hover:text-blue-400 hover:bg-gray-100 dark:hover:bg-gray-800 rounded transition-colors"
                          title="复制响应"
                        >
                          {copiedResponse[providerId] ? (
                            <>
                              <CheckCheck className="h-3.5 w-3.5" />
                              <span>已复制</span>
                            </>
                          ) : (
                            <>
                              <Copy className="h-3.5 w-3.5" />
                              <span>复制</span>
                            </>
                          )}
                        </button>
                      </div>
                    </div>
                    <div className="flex-1 overflow-hidden">
                      <pre className="text-sm text-gray-800 dark:text-gray-200 whitespace-pre-wrap font-mono break-all overflow-y-auto max-h-[280px] leading-relaxed p-3 bg-gray-50 dark:bg-gray-800 rounded">
                        {testResults[providerId].response}
                      </pre>
                    </div>
                  </div>
                )}
              </div>
            </div>
          </div>
        )}
      </div>
    );
  };

  // 列表页不再提供 Claude 插件按钮，统一在“设置”中控制

  // 对供应商列表进行搜索和排序
  const sortedProviders = useMemo(() => {
    let providerList = Object.values(providers);

    // 先进行搜索过滤
    if (searchKeyword.trim()) {
      const keyword = searchKeyword.toLowerCase().trim();
      providerList = providerList.filter(provider => {
        // 搜索供应商名称
        if (provider.name.toLowerCase().includes(keyword)) {
          return true;
        }
        // 搜索API地址
        const apiUrl = getApiUrl(provider).toLowerCase();
        if (apiUrl.includes(keyword)) {
          return true;
        }
        return false;
      });
    }

    // 按延迟排序
    if (sortMode === 'latency') {
      return providerList.sort((a, b) => {
        const stateA = testStates[a.id];
        const stateB = testStates[b.id];

        // 优先显示测试成功的供应商
        const successA = stateA?.status === "success";
        const successB = stateB?.status === "success";

        if (successA && !successB) return -1;
        if (!successA && successB) return 1;

        // 都成功时,按延迟排序(延迟低的在前)
        if (successA && successB) {
          const latencyA = stateA?.latencyMs ?? Infinity;
          const latencyB = stateB?.latencyMs ?? Infinity;
          return latencyA - latencyB;
        }

        // 都不成功时,按创建时间排序
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
    }

    // 按错误排序
    if (sortMode === 'error') {
      return providerList.sort((a, b) => {
        const stateA = testStates[a.id];
        const stateB = testStates[b.id];

        // 优先显示测试失败的供应商
        const errorA = stateA?.status === "error";
        const errorB = stateB?.status === "error";

        if (errorA && !errorB) return -1;
        if (!errorA && errorB) return 1;

        // 都失败时,按状态码排序(状态码高的在前,表示更严重的错误)
        if (errorA && errorB) {
          const codeA = stateA?.statusCode ?? 0;
          const codeB = stateB?.statusCode ?? 0;
          return codeB - codeA;
        }

        // 都不是错误时,按创建时间排序
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
    }

    // 默认按创建时间排序
    return providerList.sort((a, b) => {
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
  }, [providers, i18n.language, sortMode, testStates, searchKeyword]);

  return (
    <div className="space-y-4">
      {Object.keys(providers).length === 0 ? (
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
              {/* 搜索框 */}
              <div className="relative">
                <div className="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
                  <Search className="h-4 w-4 text-gray-400" />
                </div>
                <input
                  type="text"
                  value={searchKeyword}
                  onChange={(e) => setSearchKeyword(e.target.value)}
                  placeholder={t("provider.search.placeholder")}
                  className="w-48 pl-9 pr-8 py-2 text-sm border border-gray-300 dark:border-gray-600 rounded-md bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 placeholder-gray-500 dark:placeholder-gray-400 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent transition-all duration-200"
                />
                {searchKeyword && (
                  <button
                    onClick={() => setSearchKeyword('')}
                    className="absolute inset-y-0 right-0 pr-2 flex items-center text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 transition-colors"
                    title={t("provider.search.clearSearch")}
                  >
                    <X className="h-4 w-4" />
                  </button>
                )}
              </div>

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
              {/* 排序切换按钮 */}
              {!isBatchMode && (
                <button
                  onClick={() => {
                    // 循环切换: default -> latency -> error -> default
                    if (sortMode === 'default') {
                      setSortMode('latency');
                    } else if (sortMode === 'latency') {
                      setSortMode('error');
                    } else {
                      setSortMode('default');
                    }
                  }}
                  className={cn(
                    "inline-flex items-center gap-2 px-4 py-2 text-sm font-medium rounded-md transition-colors",
                    sortMode === 'latency'
                      ? "bg-purple-500 text-white hover:bg-purple-600 dark:bg-purple-600 dark:hover:bg-purple-700"
                      : sortMode === 'error'
                      ? "bg-red-500 text-white hover:bg-red-600 dark:bg-red-600 dark:hover:bg-red-700"
                      : "bg-gray-200 text-gray-700 hover:bg-gray-300 dark:bg-gray-700 dark:text-gray-300 dark:hover:bg-gray-600"
                  )}
                  title={
                    sortMode === 'latency'
                      ? "按延迟排序(点击切换到错误排序)"
                      : sortMode === 'error'
                      ? "按错误排序(点击恢复顺序排序)"
                      : "按添加顺序排序(点击切换到延迟排序)"
                  }
                >
                  {sortMode === 'latency' ? (
                    <>
                      <Clock className="h-4 w-4" />
                      <span>延迟排序</span>
                    </>
                  ) : sortMode === 'error' ? (
                    <>
                      <CircleAlert className="h-4 w-4" />
                      <span>错误排序</span>
                    </>
                  ) : (
                    <>
                      <ArrowUpDown className="h-4 w-4" />
                      <span>顺序排序</span>
                    </>
                  )}
                </button>
              )}

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

          {/* 搜索无结果提示 */}
          {sortedProviders.length === 0 && searchKeyword.trim() ? (
            <div className="text-center py-12">
              <div className="w-16 h-16 mx-auto mb-4 bg-gray-100 dark:bg-gray-800 rounded-full flex items-center justify-center">
                <Search size={24} className="text-gray-400" />
              </div>
              <h3 className="text-lg font-medium text-gray-900 dark:text-gray-100 mb-2">
                {t("provider.search.noResults")}
              </h3>
              <p className="text-gray-500 dark:text-gray-400 text-sm mb-4">
                {t("provider.search.noResultsDescription")} "<span className="font-semibold">{searchKeyword}</span>"
              </p>
              <button
                onClick={() => setSearchKeyword('')}
                className="inline-flex items-center gap-2 px-4 py-2 text-sm font-medium rounded-md bg-blue-500 text-white hover:bg-blue-600 dark:bg-blue-600 dark:hover:bg-blue-700 transition-colors"
              >
                {t("provider.search.clearSearch")}
              </button>
            </div>
          ) : (
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
                    <div className="flex justify-between">
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
                        <div className="flex items-center justify-between mb-2">
                          <h4 className="text-xs font-medium text-gray-700 dark:text-gray-300 flex items-center gap-1">
                            <Bug className="h-3 w-3" />
                            连接诊断信息
                          </h4>
                          <button
                            onClick={() => setCollapsedDiagnostics(prev => ({
                              ...prev,
                              [provider.id]: !prev[provider.id]
                            }))}
                            className="text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200 transition-colors"
                            title={collapsedDiagnostics[provider.id] ? "展开" : "收起"}
                          >
                            {collapsedDiagnostics[provider.id] ? (
                              <ChevronDown className="h-4 w-4" />
                            ) : (
                              <ChevronUp className="h-4 w-4" />
                            )}
                          </button>
                        </div>

                        {!collapsedDiagnostics[provider.id] && (
                          <>
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
                          </>
                        )}
                      </div>
                    )}
                  </div>
                </div>
              </div>
            );
          })}
            </div>
          )}
        </>
      )}
    </div>
  );
};

export default ProviderList;
