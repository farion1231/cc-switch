import type { TFunction } from "i18next";

/** WorkBuddy 版型 / 套餐：已知 code 本地化（authCenter.workbuddy.plan.<code>），未知的原样显示。 */
export function workbuddyPlanLabel(t: TFunction, code: string): string {
  return String(
    t([`authCenter.workbuddy.plan.${code}`, "authCenter.workbuddy.plan._raw"], {
      defaultValue: code,
      plan: code,
    }),
  );
}
