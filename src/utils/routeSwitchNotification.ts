import type { TFunction } from "i18next";
import type { AppId } from "@/lib/api/types";
import type { RouteSwitchResult } from "@/types/proxy";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";

/** Routing takes effect independently of client config loaded at startup. */
export function routeSwitchNotification(
  result: RouteSwitchResult,
  app: AppId,
  provider: string,
  t: TFunction,
): string {
  let message: string;
  switch (result.kind) {
    case "remembered":
      message = t("mode.toast.routeRemembered", {
        provider,
        defaultValue: "已记住供应商 {{provider}}，客户端接入路由或聚合后使用。",
      });
      break;
    case "route":
      message = result.clientConfigChanged
        ? t("mode.toast.routeTargetChanged", {
            provider,
            defaultValue: "路由目标已设为 {{provider}}。",
          })
        : t("mode.toast.routeSwitched", {
            provider,
            defaultValue:
              "路由已切换至 {{provider}}，后续新请求将使用该供应商。",
          });
      break;
    case "stack":
      message = t("mode.toast.stackDefaultChanged", {
        provider,
        defaultValue:
          "聚合默认供应商已设为 {{provider}}；已选聚合模型仍使用各自的供应商。",
      });
      break;
  }
  if (
    result.kind !== "remembered" &&
    result.clientConfigChanged &&
    (app === "codex" ||
      app === "gemini" ||
      app === "grokbuild" ||
      app === "claude-desktop")
  ) {
    message += t("mode.toast.clientConfigChanged", {
      app: APP_DISPLAY_NAME[app],
      defaultValue: "客户端配置也已更新；重启 {{app}} 以加载更新后的配置。",
    });
  }
  return message;
}
