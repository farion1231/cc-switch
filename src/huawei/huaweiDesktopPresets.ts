/**
 * 华为 CodeAgent 的 Claude Desktop 预设定义
 *
 * 从 claudeDesktopProviderPresets.ts 中抽离，避免与 main 分支合并冲突。
 * 由 huawei/index.ts 在模块加载时 push 到主预设数组中。
 */
import type { ClaudeDesktopProviderPreset } from "@/config/claudeDesktopProviderPresets";
import { CLAUDE_DESKTOP_ROLE_ROUTE_IDS } from "@/config/claudeDesktopProviderPresets";

function brandedRoutes(
  sonnet: string,
  opus: string,
  haiku: string,
  supports1m = false,
) {
  return [
    {
      routeId: CLAUDE_DESKTOP_ROLE_ROUTE_IDS.sonnet,
      upstreamModel: sonnet,
      supports1m,
    },
    {
      routeId: CLAUDE_DESKTOP_ROLE_ROUTE_IDS.opus,
      upstreamModel: opus,
      supports1m,
    },
    {
      routeId: CLAUDE_DESKTOP_ROLE_ROUTE_IDS.haiku,
      upstreamModel: haiku,
      supports1m,
    },
  ];
}

export const huaweiDesktopPresets: ClaudeDesktopProviderPreset[] = [
  {
    name: "Huawei CodeAgent 2.0",
    websiteUrl: "https://rnd-idea-api.huawei.com",
    apiKeyUrl: "https://id.huawei.com",
    category: "cloud_provider",
    baseUrl:
      "https://snapengine.cida.cce.prod-szv-g.dragon.tools.huawei.com/api/v2",
    mode: "proxy",
    apiFormat: "anthropic",
    providerType: "huawei_codeagent_2",
    requiresOAuth: true,
    modelRoutes: brandedRoutes(
      "claude-sonnet-4.6",
      "claude-opus-4",
      "claude-haiku-4.5",
      true,
    ),
    appTarget: "claude",
    icon: "huawei",
    iconColor: "#e63946",
  },
  {
    name: "Huawei CodeAgent 3.0",
    websiteUrl: "https://rnd-idea-api.huawei.com",
    apiKeyUrl: "https://id.huawei.com",
    category: "cloud_provider",
    baseUrl:
      "https://snapengine.cida.cce.prod-szv-g.dragon.tools.huawei.com/api/v2",
    mode: "proxy",
    apiFormat: "anthropic",
    providerType: "huawei_codeagent_3",
    requiresOAuth: true,
    modelRoutes: brandedRoutes(
      "maas-glm-5.1-zhipu",
      "maas-glm-5.1-zhipu",
      "maas-glm-5.1-zhipu",
      true,
    ),
    appTarget: "claude",
    icon: "huawei",
    iconColor: "#e63946",
  },
];
