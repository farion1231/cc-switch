/**
 * 华为 CodeAgent 供应商类型常量
 *
 * 从主 constants.ts 中抽离，避免与 main 分支的 PROVIDER_TYPES 合并冲突。
 * 运行时通过 index.ts 注入到主系统的 OAUTH_PROVIDER_TYPES 等数组中。
 */
import { OAUTH_PROVIDER_TYPES, PROVIDER_TYPES } from "@/config/constants";

export const HUAWEI_PROVIDER_TYPES_CONST = {
  HUAWEI_IDEA: "huawei_idea",
  HUAWEI_CODEAGENT_2: "huawei_codeagent_2",
  HUAWEI_CODEAGENT_3: "huawei_codeagent_3",
  HUAWEI_W3: "huawei_w3",
} as const;

export const HUAWEI_PROVIDER_TYPES: readonly string[] = [
  HUAWEI_PROVIDER_TYPES_CONST.HUAWEI_IDEA,
  HUAWEI_PROVIDER_TYPES_CONST.HUAWEI_CODEAGENT_2,
  HUAWEI_PROVIDER_TYPES_CONST.HUAWEI_CODEAGENT_3,
  HUAWEI_PROVIDER_TYPES_CONST.HUAWEI_W3,
];

/** 判断某 providerType 是否为华为供应商（CLI/Desktop 表单据此写 appTarget）。 */
export function isHuaweiProviderType(
  providerType: string | null | undefined,
): boolean {
  return (
    providerType != null && HUAWEI_PROVIDER_TYPES.includes(providerType)
  );
}

/**
 * 将华为的 4 个 providerType 注入到主系统的 OAUTH_PROVIDER_TYPES 数组中。
 * 由 huawei/index.ts 在模块加载时调用，确保 main 系统的 isOAuthProviderType() 能识别华为类型。
 */
export function injectHuaweiProviderTypes(): void {
  for (const type of HUAWEI_PROVIDER_TYPES) {
    if (!OAUTH_PROVIDER_TYPES.includes(type)) {
      (OAUTH_PROVIDER_TYPES as string[]).push(type);
    }
  }
  // 同时把常量合并到 PROVIDER_TYPES（运行时扩展，不影响类型推导）
  Object.assign(PROVIDER_TYPES, HUAWEI_PROVIDER_TYPES_CONST);
}
