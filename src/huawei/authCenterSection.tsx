import { useTranslation } from "react-i18next";
import { ProviderIcon } from "@/components/ProviderIcon";
import { HuaweiCodeAgentOAuthSection } from "./HuaweiCodeAgentOAuthSection";

export function HuaweiCodeAgentAuthCenterSection() {
  const { t } = useTranslation();

  return (
    <section className="rounded-xl border border-border/60 bg-card/60 p-6">
      <div className="mb-4 flex items-center gap-3">
        <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-muted">
          <ProviderIcon icon="generic" name="Huawei" size={20} />
        </div>
        <div>
          <h4 className="font-medium">
            {t("huaweiCodeAgent2.authCenterTitle", { defaultValue: "华为 CodeAgent" })}
          </h4>
          <p className="text-sm text-muted-foreground">
            {t("huaweiCodeAgent2.authCenterDescription", {
              defaultValue: "管理华为 CodeAgent 账号",
            })}
          </p>
        </div>
      </div>

      <HuaweiCodeAgentOAuthSection />
    </section>
  );
}

export default HuaweiCodeAgentAuthCenterSection;
