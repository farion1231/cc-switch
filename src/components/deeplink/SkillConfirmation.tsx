import { useTranslation } from "react-i18next";
import { DeepLinkImportRequest } from "../../lib/api/deeplink";

export function SkillConfirmation({
  request,
}: {
  request: DeepLinkImportRequest;
}) {
  const { t } = useTranslation();

  return (
    <div className="space-y-4">
      <h3 className="text-lg font-semibold">{t("deeplink.skill.title")}</h3>

      <div>
        <label className="block text-sm font-medium text-fg-2">
          {t("deeplink.skill.repo")}
        </label>
        <div className="mt-1 text-sm font-mono bg-subtle p-2 rounded border">
          {request.repo}
        </div>
      </div>

      <div>
        <label className="block text-sm font-medium text-fg-2">
          {t("deeplink.skill.directory")}
        </label>
        <div className="mt-1 text-sm font-mono bg-subtle p-2 rounded border">
          {request.directory}
        </div>
      </div>

      <div>
        <label className="block text-sm font-medium text-fg-2">
          {t("deeplink.skill.branch")}
        </label>
        <div className="mt-1 text-sm">{request.branch || "main"}</div>
      </div>

      <div className="text-fg-1 text-sm bg-subtle p-3 rounded border border-border-strong">
        <p>ℹ️ {t("deeplink.skill.hint")}</p>
        <p className="mt-1">{t("deeplink.skill.hintDetail")}</p>
      </div>
    </div>
  );
}
