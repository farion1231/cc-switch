import React, { useEffect } from "react";
import GenericSpeedTest, { EndpointTest } from "./GenericSpeedTest";
import { AppType } from "../lib/tauri-api";

interface CodexPackyCodeSpeedTestProps {
  providerName: string;
  baseUrl: string;
  onUpdateBaseUrl: (url: string) => void;
  appType: AppType;
  onSpeedTestRequired?: () => Promise<void>;
}

// 预设的 Codex 端点
const PRESET_CODEX_ENDPOINTS: EndpointTest[] = [
  {
    name: "默认节点",
    url: "https://codex-api.packycode.com/v1",
    latency: null,
  },
  {
    name: "香港CN2",
    url: "https://codex-api-hk-cn2.packycode.com/v1",
    latency: null,
  },
  {
    name: "香港CDN",
    url: "https://codex-api-hk-cdn.packycode.com/v1",
    latency: null,
  },
];

export const CodexPackyCodeSpeedTest: React.FC<
  CodexPackyCodeSpeedTestProps
> = ({
  providerName,
  baseUrl,
  onUpdateBaseUrl,
  appType,
  onSpeedTestRequired,
}) => {
  // 判断是否为 PackyCode 供应商
  const isPackyCodeProvider = providerName.toLowerCase().includes("packycode");

  // 如果不是 PackyCode 供应商或不是 Codex 应用，不显示测速组件
  if (!isPackyCodeProvider || appType !== "codex") {
    return null;
  }

  // 暴露测速功能供父组件调用
  useEffect(() => {
    if (onSpeedTestRequired) {
      window.codexSpeedTest = async () => {
        // 这里可以添加自动测速逻辑
        // 由于使用了通用组件，自动测速功能已经内置
      };
    }
    return () => {
      if (window.codexSpeedTest) {
        delete window.codexSpeedTest;
      }
    };
  }, [onSpeedTestRequired]);

  return (
    <GenericSpeedTest
      title="PackyCode Codex 节点测速"
      presetEndpoints={PRESET_CODEX_ENDPOINTS}
      currentUrl={baseUrl}
      onUpdateUrl={onUpdateBaseUrl}
      autoSelectBest={true}
      allowCustomEndpoints={true}
      customEndpointPlaceholder="输入自定义 Codex API 地址（如: https://your-codex-api.com/v1）"
      autoTestOnSave={true}
    />
  );
};

// 扩展 Window 接口以支持全局测速函数
declare global {
  interface Window {
    codexSpeedTest?: () => Promise<void>;
  }
}
