import { useTranslation } from "react-i18next";
import { ManagedBoundAccountLine } from "./shared/ManagedBoundAccountLine";

/** 绑了账号的 Grok 官方卡副标题 */
export function GrokBoundAccountLine({
  plan,
  ...props
}: {
  accountId: string;
  providerName: string;
  note?: string;
  /** 订阅套餐名（如「SuperGrok」「SuperGrok Heavy」）；额度未启用或未查到时不传 */
  plan?: string | null;
}) {
  const { t } = useTranslation();
  return (
    <ManagedBoundAccountLine
      {...props}
      authProvider="xai_oauth"
      accountNoun={t("provider.form.grokAccount.xaiAccount", {
        defaultValue: "xAI 账号",
      })}
      plan={plan ? () => plan : undefined}
    />
  );
}
