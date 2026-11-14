import { useTranslation } from "react-i18next";

/**
 * 应用实际在直连时，这张卡和直连的关系：`inUse` 是正在直连用的那张（保存即写进客户端），
 * `switch` 是别的卡或新卡（切换时会被要求先开路由）。不在直连时不传。
 */
export type DirectModeFormatNotice = "inUse" | "switch";

/** 上游格式要靠路由转换、应用却在直连时，格式下面的后果提示（Claude Code、Codex 共用）。 */
export function DirectModeFormatWarning({
  notice,
}: {
  notice: DirectModeFormatNotice;
}) {
  const { t } = useTranslation();
  return (
    <p className="text-xs leading-relaxed text-warning-text">
      {notice === "inUse"
        ? t("providerForm.directModeFormatInUse", {
            defaultValue:
              "这张卡正在直连使用：改成这种格式后保存，请求会失败。请先切到路由或聚合。",
          })
        : t("providerForm.directModeFormatSwitch", {
            defaultValue:
              "直连模式下用不了这种格式，切到路由或聚合后才能用这张卡。",
          })}
    </p>
  );
}
