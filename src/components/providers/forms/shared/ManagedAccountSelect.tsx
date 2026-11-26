import React from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { AlertTriangle } from "lucide-react";
import {
  managedAuthStatusQuery,
  xaiAuthStatusQuery,
} from "@/lib/query/managedAuth";
import type { ManagedAuthAccount, ManagedAuthProvider } from "@/lib/api/auth";

interface ManagedBoundAccountLineProps {
  authProvider: ManagedAuthProvider;
  accountId: string;
  providerName: string;
  note?: string;
  /** 卡名里已经有账号时代替账号写的名称，如「Claude 账号」 */
  accountNoun: string;
  /** 默认看 `requires_reauth`；Claude 另看登录过期 */
  needsReauth?: (account: ManagedAuthAccount) => boolean;
  /** 跟在账号后面的套餐，如「Max 20x」 */
  plan?: (account: ManagedAuthAccount) => string | null;
  /** 不需要重新登录时跟在后面的提示 */
  hint?: React.ReactNode;
}

/**
 * 绑了账号的官方卡副标题（官网地址的位置），Claude / Grok 官方卡共用，和 Codex 官方卡同一套
 * 写法：写账号，有备注写备注，卡名里已经有账号就写 `accountNoun`；账号失效或不在这台电脑上时
 * 跟一句提示。
 */
export function ManagedBoundAccountLine({
  authProvider,
  accountId,
  providerName,
  note,
  accountNoun,
  needsReauth = (account) => account.requires_reauth,
  plan,
  hint,
}: ManagedBoundAccountLineProps) {
  const { t } = useTranslation();
  const { data, isSuccess } = useQuery(
    authProvider === "xai_oauth"
      ? xaiAuthStatusQuery
      : managedAuthStatusQuery(authProvider),
  );
  if (!isSuccess) return null;
  const account = data?.accounts.find((a) => a.id === accountId);
  if (!account) {
    return (
      <div className="mt-0.5 flex min-w-0 items-center gap-1 text-caption text-warning-text">
        <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
        <span className="min-w-0 truncate">
          {t("provider.form.managedAccount.missing", {
            defaultValue: "不在这台电脑上的账号",
          })}
        </span>
      </div>
    );
  }
  const planLabel = plan?.(account) ?? null;
  const name = providerName.trim();
  const label =
    note ??
    (name === account.login || name.includes(account.login)
      ? accountNoun
      : account.login);
  return (
    <div className="mt-0.5 flex min-w-0 items-center gap-2 text-caption text-fg-2">
      <span className="min-w-0 truncate" title={note ?? account.login}>
        {label}
      </span>
      {/* 跟在账号后面：「me@example.com · Max 20x」（抵掉一半间距，和「 · 」一样宽） */}
      {planLabel && <span className="-ms-1 shrink-0">· {planLabel}</span>}
      {needsReauth(account) ? (
        <span className="inline-flex shrink-0 items-center gap-1 text-warning-text">
          <AlertTriangle className="h-3.5 w-3.5" />
          {t("provider.form.managedAccount.reauthBadge", {
            defaultValue: "需要重新登录",
          })}
        </span>
      ) : hint ? (
        <span className="shrink-0">{hint}</span>
      ) : null}
    </div>
  );
}
