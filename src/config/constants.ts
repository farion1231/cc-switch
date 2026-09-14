// Provider 类型常量
export const PROVIDER_TYPES = {
  GITHUB_COPILOT: "github_copilot",
  CODEX_OAUTH: "codex_oauth",
  XAI_OAUTH: "xai_oauth",
} as const;

// 托管 OAuth 供应商类型：真实凭据由本地代理按请求注入，因此无论上游是否
// 需要格式转换，都必须开启路由接管才能通过认证。新增此类预设时只需把
// providerType 加进本数组，needsRouting 判定即自动覆盖，无需逐个特判。
export const OAUTH_PROVIDER_TYPES: readonly string[] = [
  PROVIDER_TYPES.GITHUB_COPILOT,
  PROVIDER_TYPES.CODEX_OAUTH,
  PROVIDER_TYPES.XAI_OAUTH,
];

/**
 * Current builds are distributed as unsigned manual downloads. Keep the
 * updater code in place for a future signed release, but do not probe a
 * missing latest.json on every startup in the meantime.
 */
export const APP_AUTO_UPDATE_ENABLED = false;
export const APP_RELEASES_URL = "https://github.com/jixiwen/cc-switch/releases";

/** 判断某 providerType 是否为托管 OAuth（凭据由代理注入、必须开启路由）。 */
export function isOAuthProviderType(
  providerType: string | null | undefined,
): boolean {
  return providerType != null && OAUTH_PROVIDER_TYPES.includes(providerType);
}

// 用量脚本模板类型常量
export const TEMPLATE_TYPES = {
  CUSTOM: "custom",
  GENERAL: "general",
  NEW_API: "newapi",
  GITHUB_COPILOT: "github_copilot",
  TOKEN_PLAN: "token_plan",
  BALANCE: "balance",
  OFFICIAL_SUBSCRIPTION: "official_subscription",
} as const;

export type TemplateType = (typeof TEMPLATE_TYPES)[keyof typeof TEMPLATE_TYPES];
