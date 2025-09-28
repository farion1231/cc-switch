import React, { useState, useEffect } from "react";
import { Loader2, Zap, Check, X, WifiOff } from "lucide-react";
import {
  isOnline,
  formatLatency,
  testMultipleEndpoints,
} from "../../lib/speedTest";

interface SubOption {
  name: string;
  endpoints: string[];
  enableAutoSpeed?: boolean;
}

interface SubOptionSelectorProps {
  subOptions: SubOption[];
  selectedOption: string;
  selectedEndpoint: string;
  onOptionChange: (optionName: string) => void;
  onEndpointChange: (endpoint: string) => void;
}

interface EndpointTestResult {
  endpoint: string;
  latency: number;
  success: boolean;
}

const SubOptionSelector: React.FC<SubOptionSelectorProps> = ({
  subOptions,
  selectedOption,
  selectedEndpoint,
  onOptionChange,
  onEndpointChange,
}) => {
  const [testing, setTesting] = useState(false);
  const [testResults, setTestResults] = useState<EndpointTestResult[]>([]);
  const [autoSelectEnabled, setAutoSelectEnabled] = useState(false);

  // 获取当前选中选项的端点列表
  const currentOption = subOptions.find((opt) => opt.name === selectedOption);
  const currentEndpoints = currentOption?.endpoints || [];
  const enableAutoSpeed = currentOption?.enableAutoSpeed ?? false;

  // 当选项改变时，自动选择第一个端点
  useEffect(() => {
    if (
      currentEndpoints.length > 0 &&
      !currentEndpoints.includes(selectedEndpoint)
    ) {
      onEndpointChange(currentEndpoints[0]);
    }
  }, [selectedOption, currentEndpoints]);

  // 测速功能
  const handleSpeedTest = async () => {
    if (!currentEndpoints.length || testing) return;

    // 检查网络连接
    if (!isOnline()) {
      console.warn("网络连接不可用，跳过测速");
      return;
    }

    setTesting(true);
    setTestResults([]);

    try {
      const results = await testMultipleEndpoints(currentEndpoints);
      setTestResults(results);

      // 如果启用了自动选择，选择最快的成功节点
      if (autoSelectEnabled) {
        const fastest = results
          .filter((r: EndpointTestResult) => r.success)
          .sort(
            (a: EndpointTestResult, b: EndpointTestResult) =>
              a.latency - b.latency,
          )[0];

        if (fastest) {
          onEndpointChange(fastest.endpoint);
        }
      }
    } catch (error) {
      console.error("测速失败:", error);
      // 创建失败结果
      const failureResults = currentEndpoints.map((endpoint) => ({
        endpoint,
        latency: 10000,
        success: false,
        error: "测速服务异常",
      }));
      setTestResults(failureResults);
    } finally {
      setTesting(false);
    }
  };

  // 获取端点的测试结果
  const getEndpointResult = (
    endpoint: string,
  ): EndpointTestResult | undefined => {
    return testResults.find((r) => r.endpoint === endpoint);
  };

  // 网络状态检查
  const networkOnline = isOnline();

  if (subOptions.length === 0) {
    return null;
  }

  return (
    <div className="space-y-4">
      {/* 二级选项选择 - 只有在多个选项时才显示 */}
      {subOptions.length > 1 && (
        <div>
          <label className="block text-sm font-medium text-gray-900 dark:text-gray-100 mb-2">
            选择线路类型
          </label>
          <div className="flex gap-2">
            {subOptions.map((option) => (
              <button
                key={option.name}
                type="button"
                className={`px-4 py-2 rounded-lg text-sm font-medium transition-colors ${
                  selectedOption === option.name
                    ? "bg-blue-500 text-white"
                    : "bg-gray-100 dark:bg-gray-700 text-gray-700 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-gray-600"
                }`}
                onClick={() => onOptionChange(option.name)}
              >
                {option.name}
              </button>
            ))}
          </div>
        </div>
      )}

      {/* 端点选择 */}
      <div>
        <div className="flex items-center justify-between mb-2">
          <label className="block text-sm font-medium text-gray-900 dark:text-gray-100">
            选择节点
          </label>
          {enableAutoSpeed && (
            <div className="flex items-center gap-2">
              <label className="flex items-center gap-2 text-sm text-gray-700 dark:text-gray-300">
                <input
                  type="checkbox"
                  checked={autoSelectEnabled}
                  onChange={(e) => setAutoSelectEnabled(e.target.checked)}
                  className="rounded border-gray-300 dark:border-gray-600"
                />
                自动选择最快节点
              </label>
              <button
                type="button"
                onClick={handleSpeedTest}
                disabled={testing || !networkOnline}
                className={`inline-flex items-center gap-1 px-3 py-1 text-sm font-medium rounded transition-colors ${
                  testing || !networkOnline
                    ? "text-gray-400 dark:text-gray-500 cursor-not-allowed"
                    : "text-blue-600 dark:text-blue-400 hover:text-blue-700 dark:hover:text-blue-300 hover:bg-blue-50 dark:hover:bg-blue-900/20"
                }`}
                title={!networkOnline ? "网络连接不可用" : "测试所有节点延迟"}
              >
                {!networkOnline ? (
                  <>
                    <WifiOff size={14} />
                    离线
                  </>
                ) : testing ? (
                  <>
                    <Loader2 size={14} className="animate-spin" />
                    测速中...
                  </>
                ) : (
                  <>
                    <Zap size={14} />
                    测速
                  </>
                )}
              </button>
            </div>
          )}
        </div>

        <select
          value={selectedEndpoint}
          onChange={(e) => onEndpointChange(e.target.value)}
          className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 dark:bg-gray-800 dark:text-gray-100 rounded-lg focus:ring-2 focus:ring-blue-500 dark:focus:ring-blue-400 focus:border-blue-500 dark:focus:border-blue-400"
        >
          {currentEndpoints.map((endpoint) => {
            const result = getEndpointResult(endpoint);
            return (
              <option key={endpoint} value={endpoint}>
                {endpoint}
                {result &&
                  (result.success
                    ? ` - ${formatLatency(result.latency)}`
                    : " - 失败")}
              </option>
            );
          })}
        </select>

        {/* 测速结果显示 */}
        {testResults.length > 0 && (
          <div className="mt-2 space-y-1">
            {testResults
              .sort((a, b) => {
                if (a.success && !b.success) return -1;
                if (!a.success && b.success) return 1;
                return a.latency - b.latency;
              })
              .map((result) => (
                <div
                  key={result.endpoint}
                  className={`flex items-center justify-between px-3 py-2 text-sm rounded-lg ${
                    result.endpoint === selectedEndpoint
                      ? "bg-blue-50 dark:bg-blue-900/20 border border-blue-200 dark:border-blue-800"
                      : "bg-gray-50 dark:bg-gray-800"
                  }`}
                >
                  <span className="font-medium truncate flex-1 text-gray-900 dark:text-gray-100">
                    {result.endpoint}
                  </span>
                  <span className="flex items-center gap-1 ml-2">
                    {result.success ? (
                      <>
                        <Check
                          size={14}
                          className="text-green-500 dark:text-green-400"
                        />
                        <span className="text-green-600 dark:text-green-400">
                          {formatLatency(result.latency)}
                        </span>
                      </>
                    ) : (
                      <>
                        <X
                          size={14}
                          className="text-red-500 dark:text-red-400"
                        />
                        <span className="text-red-600 dark:text-red-400">
                          失败
                        </span>
                      </>
                    )}
                  </span>
                </div>
              ))}
          </div>
        )}
      </div>
    </div>
  );
};

export default SubOptionSelector;
