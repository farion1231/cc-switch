import type { TFunction } from "i18next";

/**
 * ChatGPT 订阅套餐的显示名：id_token 里记录的 `chatgpt_plan_type`
 * （`free` / `plus` / `pro` / `team` / `enterprise`）。认不出的套餐照原样首字母大写，
 * 读不到时为 null。注意这只是登录凭据里的套餐，不是续费到期时间。
 */
export function chatgptPlanLabel(
  t: TFunction,
  planType: string | null | undefined,
): string | null {
  const type = planType?.trim().toLowerCase();
  if (!type) return null;
  const key = "codexOauth.plan";
  switch (type) {
    case "free":
      return t(`${key}.free`, { defaultValue: "Free" });
    case "plus":
      return t(`${key}.plus`, { defaultValue: "Plus" });
    case "pro":
      return t(`${key}.pro`, { defaultValue: "Pro" });
    case "team":
      return t(`${key}.team`, { defaultValue: "Team" });
    case "enterprise":
      return t(`${key}.enterprise`, { defaultValue: "Enterprise" });
    default:
      return type.charAt(0).toUpperCase() + type.slice(1);
  }
}
