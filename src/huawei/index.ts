/**
 * 华为 CodeAgent 供应商支持模块入口
 *
 * 所有华为相关代码通过此文件在应用启动时注入到主系统：
 * 1. providerType 常量注入到 OAUTH_PROVIDER_TYPES
 * 2. CLI 预设注入到 claudeProviderPresets
 * 3. Desktop 预设注入到 claudeDesktopProviderPresets
 * 4. 表单扩展注册到 formExtensions
 * 5. 授权中心面板注册到 authCenterRegistry
 * 6. 多语言翻译注入到 i18n
 *
 * 将供应商实现集中在本模块，减少 main 更新时共享文件的冲突范围。
 */
import React from "react";
import { registerFormExtension } from "@/config/formExtensions";
import { registerAuthCenterSection } from "@/config/authCenterRegistry";
import { providerPresets } from "@/config/claudeProviderPresets";
import { claudeDesktopProviderPresets } from "@/config/claudeDesktopProviderPresets";
import {
  DEFAULT_FINGERPRINT_REPLACEMENTS,
  HUAWEI_CODEAGENT_2_AUTH_HEADERS,
} from "./huaweiCodeAgent2Presets";
import {
  huaweiCodeAgent3Preset,
  CODEAGENT_3_AUTH_HEADERS,
} from "./huaweiCodeAgent3Presets";
import { huaweiCodeAgent2Preset } from "./huaweiCodeAgent2Presets";
import { fetchHuaweiCodeAgent2Models } from "./huaweiCodeAgent2Api";
import { huaweiDesktopPresets } from "./huaweiDesktopPresets";
import { injectHuaweiProviderTypes } from "./constants";
import "./locales";
import { huaweiFormBehavior } from "./formBehavior";

// 1. 注入 providerType 常量到主系统
injectHuaweiProviderTypes();

// 2. 注入 CLI 预设
providerPresets.push(huaweiCodeAgent2Preset, huaweiCodeAgent3Preset);

// 3. 注入 Desktop 预设
claudeDesktopProviderPresets.push(...huaweiDesktopPresets);

const huaweiCodeAgent2Extension = {
  ...huaweiFormBehavior,
  skipApiKeyRequired: true,
  AuthSection: React.lazy(() =>
    import("./HuaweiCodeAgentOAuthSection").then((m) => ({
      default: m.HuaweiCodeAgentOAuthSection,
    })),
  ),
  fetchModels: fetchHuaweiCodeAgent2Models,
  defaultMetaFields: {
    skipTlsVerify: true,
    stripPathPrefix: "/v1",
    headerMode: "whitelist",
    allowedHeaders: ["content-type", "accept-encoding", "host"],
    errorFormat: "to_anthropic",
    extraAuthHeaders: HUAWEI_CODEAGENT_2_AUTH_HEADERS,
    fingerprintReplacements: DEFAULT_FINGERPRINT_REPLACEMENTS,
  },
};

const huaweiCodeAgent3Extension = {
  ...huaweiFormBehavior,
  skipApiKeyRequired: true,
  AuthSection: React.lazy(() =>
    import("./HuaweiCodeAgentOAuthSection").then((m) => ({
      default: m.HuaweiCodeAgentOAuthSection,
    })),
  ),
  fetchModels: fetchHuaweiCodeAgent2Models,
  defaultMetaFields: {
    skipTlsVerify: true,
    stripPathPrefix: "/v1",
    headerMode: "whitelist",
    allowedHeaders: ["content-type", "accept-encoding", "host"],
    errorFormat: "to_anthropic",
    extraAuthHeaders: CODEAGENT_3_AUTH_HEADERS,
    fingerprintReplacements: DEFAULT_FINGERPRINT_REPLACEMENTS,
  },
};

// 4. 注册表单扩展
registerFormExtension({
  providerType: "huawei_codeagent_2",
  ...huaweiCodeAgent2Extension,
});
registerFormExtension({
  providerType: "huawei_w3",
  ...huaweiCodeAgent2Extension,
});
registerFormExtension({
  providerType: "huawei_idea",
  ...huaweiCodeAgent2Extension,
});
registerFormExtension({
  providerType: "huawei_codeagent_3",
  ...huaweiCodeAgent3Extension,
});

// 5. 注册授权中心面板
registerAuthCenterSection({
  id: "huawei_codeagent_2",
  Component: React.lazy(() =>
    import("./authCenterSection").then((m) => ({
      default: m.HuaweiCodeAgentAuthCenterSection,
    })),
  ),
});
