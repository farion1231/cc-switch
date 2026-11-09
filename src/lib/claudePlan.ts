import type { TFunction } from "i18next";

/**
 * Claude 订阅套餐的显示名：`subscriptionType`（Claude Code 记在凭据里）加 `rateLimitTier`
 * 区分 Max 5x / 20x、Team 高级席位。认不出的套餐照原样写，读不到时为 null。
 */
export function claudePlanLabel(
  t: TFunction,
  subscriptionType: string | null | undefined,
  rateLimitTier: string | null | undefined,
): string | null {
  const type = subscriptionType?.trim().toLowerCase();
  if (!type) return null;
  const key = "provider.form.claudeAccount.plan";
  switch (type) {
    case "pro":
      return t(`${key}.pro`, { defaultValue: "Pro" });
    case "max":
      if (rateLimitTier === "default_claude_max_20x") {
        return t(`${key}.max20x`, { defaultValue: "Max 20x" });
      }
      if (rateLimitTier === "default_claude_max_5x") {
        return t(`${key}.max5x`, { defaultValue: "Max 5x" });
      }
      return t(`${key}.max`, { defaultValue: "Max" });
    case "team":
      // Claude Code 自己也按这个组合认 Team 的高级席位
      return rateLimitTier === "default_claude_max_5x"
        ? t(`${key}.teamPremium`, { defaultValue: "Team 高级席位" })
        : t(`${key}.team`, { defaultValue: "Team" });
    case "enterprise":
      return t(`${key}.enterprise`, { defaultValue: "Enterprise" });
    default:
      return type.charAt(0).toUpperCase() + type.slice(1);
  }
}
