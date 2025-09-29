import React, { useState, useEffect, useCallback } from "react";
import { Zap, Loader2, Check, AlertCircle, Plus, Trash2 } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";

// 通用端点测试接口
export interface EndpointTest {
  url: string;
  name: string;
  latency: number | null;
  isCustom?: boolean;
  metadata?: Record<string, any>; // 额外的元数据
}

// 通用测速组件属性
export interface GenericSpeedTestProps {
  // 组件标题
  title?: string;
  // 预设端点列表
  presetEndpoints?: EndpointTest[];
  // 当前选中的 URL
  currentUrl: string;
  // URL 更新回调
  onUpdateUrl: (url: string) => void;
  // 是否自动选择最佳节点
  autoSelectBest?: boolean;
  // 是否允许添加自定义端点
  allowCustomEndpoints?: boolean;
  // 自定义端点占位符文本
  customEndpointPlaceholder?: string;
  // 是否在保存时自动测速
  autoTestOnSave?: boolean;
  // 测速时的额外参数
  testParams?: Record<string, any>;
  // 自定义渲染端点信息
  renderEndpointInfo?: (endpoint: EndpointTest) => React.ReactNode;
  // 是否显示测速组件
  visible?: boolean;
}

/**
 * 通用测速组件
 * 可以被任何第三方供应商使用
 */
export const GenericSpeedTest: React.FC<GenericSpeedTestProps> = ({
  title = "节点测速",
  presetEndpoints = [],
  currentUrl,
  onUpdateUrl,
  autoSelectBest = true,
  allowCustomEndpoints = true,
  customEndpointPlaceholder = "输入自定义 API 地址",
  autoTestOnSave = true,
  testParams = {},
  renderEndpointInfo,
  visible = true,
}) => {
  const [isTestingSpeed, setIsTestingSpeed] = useState(false);
  const [testResults, setTestResults] = useState<EndpointTest[]>([]);
  const [autoSelect, setAutoSelect] = useState(autoSelectBest);
  const [customUrl, setCustomUrl] = useState("");
  const [customEndpoints, setCustomEndpoints] = useState<EndpointTest[]>([]);
  const [hasTestedOnce, setHasTestedOnce] = useState(false);

  // 如果不可见，不渲染组件
  if (!visible) {
    return null;
  }

  // 测试单个端点
  const testSingleEndpoint = async (
    endpoint: EndpointTest,
  ): Promise<EndpointTest> => {
    try {
      const result = await invoke<{ latency: number | null }>(
        "test_single_endpoint",
        {
          url: endpoint.url,
          ...testParams,
        },
      );
      return { ...endpoint, latency: result.latency };
    } catch (error) {
      console.error(`测试端点 ${endpoint.url} 失败:`, error);
      return { ...endpoint, latency: null };
    }
  };

  // 批量测试端点（使用后端并发接口）
  const testEndpointsBatch = async (
    endpoints: EndpointTest[],
  ): Promise<EndpointTest[]> => {
    try {
      const urls = endpoints.map((ep) => ep.url);
      const results = await invoke<
        Array<{ url: string; latency: number | null }>
      >("test_endpoints_batch", { urls });

      // 将结果映射回端点
      return endpoints.map((endpoint) => {
        const result = results.find((r) => r.url === endpoint.url);
        return {
          ...endpoint,
          latency: result?.latency ?? null,
        };
      });
    } catch (error) {
      console.error("批量测速失败，降级到单个测试:", error);
      // 如果批量测速失败，降级到单个测试
      return Promise.all(
        endpoints.map((endpoint) => testSingleEndpoint(endpoint)),
      );
    }
  };

  // 执行测速
  const performSpeedTest = useCallback(
    async (silent = false) => {
      if (!silent) setIsTestingSpeed(true);

      try {
        // 合并预设和自定义端点
        const allEndpoints = [...presetEndpoints, ...customEndpoints];

        // 使用批量并发测试
        const results = await testEndpointsBatch(allEndpoints);

        // 按延迟从低到高排序
        const sortedResults = results.sort((a, b) => {
          if (a.latency === null) return 1;
          if (b.latency === null) return -1;
          return a.latency - b.latency;
        });

        setTestResults(sortedResults);
        setHasTestedOnce(true);

        // 如果开启自动选择，选择延迟最低的端点
        if (autoSelect) {
          let bestEndpoint: EndpointTest | null = null;
          let minLatency = Infinity;

          for (const endpoint of results) {
            if (endpoint.latency !== null && endpoint.latency < minLatency) {
              minLatency = endpoint.latency;
              bestEndpoint = endpoint;
            }
          }

          if (bestEndpoint) {
            onUpdateUrl(bestEndpoint.url);
          }
        }
      } catch (error) {
        console.error("测速失败:", error);
      } finally {
        if (!silent) setIsTestingSpeed(false);
      }
    },
    [presetEndpoints, customEndpoints, autoSelect, onUpdateUrl, testParams],
  );

  // 添加自定义端点
  const addCustomEndpoint = () => {
    if (!customUrl.trim()) return;

    // 验证 URL 格式
    try {
      const url = new URL(customUrl);
      if (!url.protocol.startsWith("http")) {
        alert("请输入有效的 HTTP/HTTPS URL");
        return;
      }
    } catch {
      alert("请输入有效的 URL");
      return;
    }

    // 检查是否已存在
    const exists = [...presetEndpoints, ...customEndpoints].some(
      (ep) => ep.url === customUrl,
    );

    if (exists) {
      alert("该地址已存在");
      return;
    }

    const newEndpoint: EndpointTest = {
      name: `自定义节点 ${customEndpoints.length + 1}`,
      url: customUrl,
      latency: null,
      isCustom: true,
    };

    setCustomEndpoints([...customEndpoints, newEndpoint]);
    setCustomUrl("");

    // 如果已经测速过，测试新添加的端点
    if (hasTestedOnce) {
      testSingleEndpoint(newEndpoint).then((result) => {
        setTestResults((prev) => {
          // 添加后重新排序
          const updated = [...prev, result];
          return updated.sort((a, b) => {
            if (a.latency === null) return 1;
            if (b.latency === null) return -1;
            return a.latency - b.latency;
          });
        });
      });
    }
  };

  // 删除自定义端点
  const removeCustomEndpoint = (url: string) => {
    setCustomEndpoints((prev) => prev.filter((ep) => ep.url !== url));
    setTestResults((prev) => prev.filter((ep) => ep.url !== url));
  };

  // 应用选定的端点
  const applyEndpoint = (url: string) => {
    onUpdateUrl(url);
  };

  // 暴露测速功能供外部调用
  useEffect(() => {
    if (autoTestOnSave) {
      // 将测速函数挂载到 window 对象，供父组件调用
      const testFunction = async () => {
        if (!hasTestedOnce) {
          await performSpeedTest(true);
        }
      };

      // 为不同的供应商使用不同的全局函数名
      const functionName = `speedTest_${Date.now()}`;
      (window as any)[functionName] = testFunction;

      // 清理函数
      return () => {
        delete (window as any)[functionName];
      };
    }
  }, [hasTestedOnce, performSpeedTest, autoTestOnSave]);

  // 获取要显示的端点列表
  const getDisplayEndpoints = () => {
    if (testResults.length > 0) {
      return testResults;
    }
    return [...presetEndpoints, ...customEndpoints];
  };

  return (
    <div className="mt-4 p-4 bg-gray-50 dark:bg-gray-800 rounded-lg">
      <div className="flex items-center justify-between mb-3">
        <h3 className="text-sm font-medium text-gray-700 dark:text-gray-300">
          {title}
        </h3>
        <button
          type="button"
          onClick={(e) => {
            e.preventDefault();
            e.stopPropagation();
            performSpeedTest(false);
          }}
          disabled={isTestingSpeed}
          className="flex items-center gap-2 px-3 py-1.5 bg-blue-500 text-white rounded-md hover:bg-blue-600 disabled:opacity-50 disabled:cursor-not-allowed text-sm"
        >
          {isTestingSpeed ? (
            <>
              <Loader2 className="w-4 h-4 animate-spin" />
              测速中...
            </>
          ) : (
            <>
              <Zap className="w-4 h-4" />
              开始测速
            </>
          )}
        </button>
      </div>

      <div className="flex items-center gap-2 mb-3">
        <input
          type="checkbox"
          id="auto-select-generic"
          checked={autoSelect}
          onChange={(e) => setAutoSelect(e.target.checked)}
          className="rounded border-gray-300 dark:border-gray-600"
        />
        <label
          htmlFor="auto-select-generic"
          className="text-sm text-gray-600 dark:text-gray-400"
        >
          自动选择延迟最低的节点
        </label>
      </div>

      {/* 自定义地址输入 */}
      {allowCustomEndpoints && (
        <div className="mb-3">
          <div className="flex gap-2">
            <input
              type="url"
              value={customUrl}
              onChange={(e) => setCustomUrl(e.target.value)}
              onKeyPress={(e) => e.key === "Enter" && addCustomEndpoint()}
              placeholder={customEndpointPlaceholder}
              className="flex-1 px-3 py-1.5 text-sm border border-gray-200 dark:border-gray-700 dark:bg-gray-900 dark:text-gray-100 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500/20 dark:focus:ring-blue-400/20 focus:border-blue-500 dark:focus:border-blue-400"
            />
            <button
              type="button"
              onClick={(e) => {
                e.preventDefault();
                e.stopPropagation();
                addCustomEndpoint();
              }}
              className="px-3 py-1.5 bg-green-500 text-white rounded-md hover:bg-green-600 text-sm flex items-center gap-1"
            >
              <Plus className="w-4 h-4" />
              添加
            </button>
          </div>
        </div>
      )}

      {/* 测速结果 */}
      {getDisplayEndpoints().length > 0 && (
        <div className="space-y-1">
          <h4 className="text-sm font-medium text-gray-600 dark:text-gray-400 mb-2">
            节点列表
          </h4>
          {getDisplayEndpoints().map((endpoint) => {
            const testResult = testResults.find((r) => r.url === endpoint.url);
            const isSelected = currentUrl === endpoint.url;
            const hasLatency =
              testResult?.latency !== null && testResult?.latency !== undefined;

            return (
              <div
                key={endpoint.url}
                className={`flex items-center justify-between p-2 rounded-md text-sm ${
                  isSelected
                    ? "bg-green-100 dark:bg-green-900/30 border border-green-300 dark:border-green-700"
                    : "bg-white dark:bg-gray-700 border border-gray-200 dark:border-gray-600"
                }`}
              >
                <div className="flex-1 flex items-center gap-2">
                  {isSelected && (
                    <Check className="w-4 h-4 text-green-600 dark:text-green-400" />
                  )}
                  <div className="flex-1">
                    {renderEndpointInfo ? (
                      renderEndpointInfo(endpoint)
                    ) : (
                      <>
                        <div className="font-medium">{endpoint.name}</div>
                        <div className="text-xs text-gray-500 dark:text-gray-400 break-all">
                          {endpoint.url}
                        </div>
                      </>
                    )}
                  </div>
                </div>
                <div className="flex items-center gap-2 ml-2">
                  {testResult ? (
                    hasLatency ? (
                      <span
                        className={`font-mono text-xs ${
                          testResult.latency! < 100
                            ? "text-green-600 dark:text-green-400"
                            : testResult.latency! < 300
                              ? "text-yellow-600 dark:text-yellow-400"
                              : "text-red-600 dark:text-red-400"
                        }`}
                      >
                        {testResult.latency}ms
                      </span>
                    ) : (
                      <span className="text-gray-400 flex items-center gap-1 text-xs">
                        <AlertCircle className="w-3 h-3" />
                        超时
                      </span>
                    )
                  ) : (
                    <span className="text-gray-400 text-xs">未测试</span>
                  )}

                  {!autoSelect && hasLatency && (
                    <button
                      type="button"
                      onClick={(e) => {
                        e.preventDefault();
                        e.stopPropagation();
                        applyEndpoint(endpoint.url);
                      }}
                      disabled={isSelected}
                      className="px-2 py-0.5 text-xs bg-blue-500 text-white rounded hover:bg-blue-600 disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                      {isSelected ? "已选择" : "选择"}
                    </button>
                  )}

                  {endpoint.isCustom && (
                    <button
                      type="button"
                      onClick={(e) => {
                        e.preventDefault();
                        e.stopPropagation();
                        removeCustomEndpoint(endpoint.url);
                      }}
                      className="p-1 text-red-500 hover:text-red-600"
                      title="删除自定义端点"
                    >
                      <Trash2 className="w-3 h-3" />
                    </button>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      )}

      {!hasTestedOnce && autoTestOnSave && (
        <div className="mt-3 p-2 bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-800 rounded-md">
          <p className="text-xs text-amber-700 dark:text-amber-300">
            💡 提示：保存时会自动测速并选择最佳节点
          </p>
        </div>
      )}
    </div>
  );
};

export default GenericSpeedTest;
