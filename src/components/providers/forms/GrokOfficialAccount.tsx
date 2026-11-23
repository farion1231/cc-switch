import { useTranslation } from "react-i18next";
import type { ManagedAuthAccount } from "@/lib/api/auth";
import {
  ManagedAccountSelect,
  ManagedBoundAccountLine,
} from "./shared/ManagedAccountSelect";

interface GrokAccountSelectProps {
  value: string | null;
  onChange: (accountId: string | null, account?: ManagedAuthAccount) => void;
  /** 打开授权中心添加或管理 xAI 账号 */
  onManageAccounts?: () => void;
}

/** Grok 官方卡的登录方式：授权中心的 xAI 账号，或跟随 Grok CLI 登录（~/.grok/auth.json，不绑号）。 */
export function GrokAccountSelect(props: GrokAccountSelectProps) {
  const { t } = useTranslation();
  return (
    <ManagedAccountSelect
      {...props}
      authProvider="xai_oauth"
      labels={{
        follow: t("provider.form.grokAccount.default", {
          defaultValue: "跟随 Grok CLI 登录",
        }),
        followDescription: t("provider.form.grokAccount.defaultDescription", {
          defaultValue: "账号会随 Grok CLI 当前登录变化",
        }),
        manage: t("provider.form.grokAccount.manage", {
          defaultValue: "添加或管理 xAI 账号",
        }),
      }}
      help={t("provider.form.grokAccount.help", {
        defaultValue:
          "切到这张卡后，Grok CLI 的登录（~/.grok/auth.json）会换成选中的账号；原来登录的号会先收进授权中心，切回「跟随 Grok CLI 登录」的卡时还原。",
      })}
    />
  );
}

/** 绑了账号的 Grok 官方卡副标题 */
export function GrokBoundAccountLine(props: {
  accountId: string;
  providerName: string;
  note?: string;
}) {
  const { t } = useTranslation();
  return (
    <ManagedBoundAccountLine
      {...props}
      authProvider="xai_oauth"
      accountNoun={t("provider.form.grokAccount.xaiAccount", {
        defaultValue: "xAI 账号",
      })}
    />
  );
}
