import React, { useState, useEffect } from "react";
import GenericSpeedTest, { EndpointTest } from "./GenericSpeedTest";

interface ClaudePackyCodeSpeedTestProps {
  providerName: string;
  baseUrl: string;
  apiKey: string;
  onUpdateBaseUrl: (url: string) => void;
  onSpeedTestRequired?: () => Promise<void>;
}

// 预设的 Claude PackyCode 端点
const CLAUDE_PACKYCODE_SERVICES = {
  公交车: [
    {
      name: "公交车 - 默认节点",
      url: "https://api.packycode.com",
      latency: null,
    },
    {
      name: "公交车 - 香港CN2",
      url: "https://api-hk-cn2.packycode.com",
      latency: null,
    },
    {
      name: "公交车 - 香港G口",
      url: "https://api-hk-g.packycode.com",
      latency: null,
    },
    {
      name: "公交车 - 美国CN2",
      url: "https://api-us-cn2.packycode.com",
      latency: null,
    },
    {
      name: "公交车 - Cloudflare Pro",
      url: "https://api-cf-pro.packycode.com",
      latency: null,
    },
  ],
  滴滴车: [
    {
      name: "滴滴车 - 默认节点",
      url: "https://share-api.packycode.com",
      latency: null,
    },
    {
      name: "滴滴车 - 香港CN2",
      url: "https://share-api-hk-cn2.packycode.com",
      latency: null,
    },
    {
      name: "滴滴车 - 香港G口",
      url: "https://share-api-hk-g.packycode.com",
      latency: null,
    },
    {
      name: "滴滴车 - 美国CN2",
      url: "https://share-api-us-cn2.packycode.com",
      latency: null,
    },
    {
      name: "滴滴车 - Cloudflare Pro",
      url: "https://share-api-cf-pro.packycode.com",
      latency: null,
    },
  ],
};

export const ClaudePackyCodeSpeedTest: React.FC<
  ClaudePackyCodeSpeedTestProps
> = ({
  providerName,
  baseUrl,
  apiKey: _apiKey, // 保留接口一致性
  onUpdateBaseUrl,
  onSpeedTestRequired,
}) => {
  const [selectedServiceType, setSelectedServiceType] = useState<
    "公交车" | "滴滴车" | null
  >(null);
  const [presetEndpoints, setPresetEndpoints] = useState<EndpointTest[]>([]);

  // 判断是否为 PackyCode 供应商
  const isPackyCodeProvider = providerName.toLowerCase().includes("packycode");

  // 如果不是 PackyCode 供应商，不显示测速组件
  if (!isPackyCodeProvider) {
    return null;
  }

  // 根据服务类型更新预设端点
  useEffect(() => {
    if (selectedServiceType) {
      setPresetEndpoints(CLAUDE_PACKYCODE_SERVICES[selectedServiceType]);
    } else {
      // 未选择时显示所有端点
      setPresetEndpoints([
        ...CLAUDE_PACKYCODE_SERVICES["公交车"],
        ...CLAUDE_PACKYCODE_SERVICES["滴滴车"],
      ]);
    }
  }, [selectedServiceType]);

  // 初始化时根据当前 baseUrl 判断服务类型
  useEffect(() => {
    if (baseUrl.includes("share-api")) {
      setSelectedServiceType("滴滴车");
    } else if (
      baseUrl.includes("api.packycode.com") ||
      baseUrl.includes("api-")
    ) {
      setSelectedServiceType("公交车");
    }
  }, []);

  // 暴露测速功能供父组件调用
  useEffect(() => {
    if (onSpeedTestRequired) {
      window.claudePackyCodeSpeedTest = async () => {
        // 如果还没有选择服务类型，默认选择公交车
        if (!selectedServiceType) {
          setSelectedServiceType("公交车");
        }
      };
    }
    return () => {
      if (window.claudePackyCodeSpeedTest) {
        delete window.claudePackyCodeSpeedTest;
      }
    };
  }, [selectedServiceType, onSpeedTestRequired]);

  return (
    <div className="space-y-4">
      {/* 服务类型选择 */}
      <div className="flex items-center gap-3">
        <span className="text-sm text-gray-600 dark:text-gray-400">
          选择服务：
        </span>
        <div className="flex gap-2">
          <button
            type="button"
            onClick={(e) => {
              e.preventDefault();
              e.stopPropagation();
              setSelectedServiceType("公交车");
            }}
            className={`px-3 py-1 text-xs rounded-md transition-colors ${
              selectedServiceType === "公交车"
                ? "bg-blue-500 text-white"
                : "bg-gray-200 dark:bg-gray-700 text-gray-700 dark:text-gray-300 hover:bg-gray-300 dark:hover:bg-gray-600"
            }`}
          >
            公交车
          </button>
          <button
            type="button"
            onClick={(e) => {
              e.preventDefault();
              e.stopPropagation();
              setSelectedServiceType("滴滴车");
            }}
            className={`px-3 py-1 text-xs rounded-md transition-colors ${
              selectedServiceType === "滴滴车"
                ? "bg-blue-500 text-white"
                : "bg-gray-200 dark:bg-gray-700 text-gray-700 dark:text-gray-300 hover:bg-gray-300 dark:hover:bg-gray-600"
            }`}
          >
            滴滴车
          </button>
        </div>
        {!selectedServiceType && (
          <span className="text-xs text-amber-600 dark:text-amber-400">
            请先选择服务类型
          </span>
        )}
      </div>

      {/* 使用通用测速组件 */}
      <GenericSpeedTest
        title="PackyCode 节点测速"
        presetEndpoints={presetEndpoints}
        currentUrl={baseUrl}
        onUpdateUrl={onUpdateBaseUrl}
        autoSelectBest={true}
        allowCustomEndpoints={true}
        customEndpointPlaceholder="输入自定义 PackyCode API 地址（如: https://your-api.packycode.com）"
        autoTestOnSave={true}
      />
    </div>
  );
};

// 扩展 Window 接口以支持全局测速函数
declare global {
  interface Window {
    claudePackyCodeSpeedTest?: () => Promise<void>;
  }
}
