import React from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { AlertTriangle, Plus } from "lucide-react";
import { authApi } from "@/lib/api";
import type { ManagedAuthAccount, ManagedAuthProvider } from "@/lib/api/auth";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

const DEFAULT_LOGIN = "__default__";
const MANAGE_ACCOUNTS = "__manage_accounts__";

/** 授权中心里某一类账号（和授权中心、各官方卡共用同一个查询缓存） */
export function useManagedAccounts(authProvider: ManagedAuthProvider) {
  return useQuery({
    queryKey: ["managed-auth-status", authProvider],
    queryFn: () => authApi.authGetStatus(authProvider),
    staleTime: 30000,
  });
}

export interface ManagedAccountSelectLabels {
  /** 「跟随 xx 登录」 */
  follow: string;
  /** 跟随登录一项后面的说明 */
  followDescription: string;
  /** 「添加或管理 xx 账号」 */
  manage: string;
}

interface ManagedAccountSelectProps {
  authProvider: ManagedAuthProvider;
  value: string | null;
  onChange: (accountId: string | null, account?: ManagedAuthAccount) => void;
  /** 打开授权中心添加或管理账号 */
  onManageAccounts?: () => void;
  labels: ManagedAccountSelectLabels;
  /** 选择框下面的说明 */
  help: React.ReactNode;
}

/**
 * 官方卡的登录方式（Claude / Grok 官方卡共用，和 Codex 官方卡同一套写法）：授权中心的账号在上，
 * 「添加或管理」打开授权中心，最下面是跟随客户端自己的登录（不绑号）。
 */
export function ManagedAccountSelect({
  authProvider,
  value,
  onChange,
  onManageAccounts,
  labels,
  help,
}: ManagedAccountSelectProps) {
  const { t } = useTranslation();
  const { data } = useManagedAccounts(authProvider);
  const accounts = data?.accounts ?? [];
  const selected = accounts.find((a) => a.id === value);
  // 绑的号不在这台电脑上（别的设备同步来的、或已删掉）时也要显示出来，别悄悄改成跟随登录
  const missing = value !== null && !selected;
  const missingLabel = t("provider.form.managedAccount.missing", {
    defaultValue: "不在这台电脑上的账号",
  });
  const label = t("provider.form.managedAccount.label", {
    defaultValue: "登录方式",
  });
  const itemClass =
    "min-w-0 overflow-hidden py-2 pl-6 [&>span:last-child]:min-w-0 [&>span:last-child]:flex-1 [&>span:last-child]:overflow-hidden";

  return (
    <div className="space-y-2.5">
      <Label className="text-sm font-medium text-fg-1">{label}</Label>
      <Select
        value={value ?? DEFAULT_LOGIN}
        onValueChange={(next) => {
          // Radix Select 在表单里可能回报空串，忽略
          if (!next) return;
          if (next === MANAGE_ACCOUNTS) {
            onManageAccounts?.();
            return;
          }
          if (next === DEFAULT_LOGIN) {
            onChange(null);
            return;
          }
          onChange(
            next,
            accounts.find((a) => a.id === next),
          );
        }}
      >
        <SelectTrigger
          className="h-10 min-w-0 rounded-lg bg-surface px-3 shadow-sm"
          aria-label={label}
        >
          <span
            className="min-w-0 flex-1 truncate text-left text-sm font-medium tracking-tight"
            title={selected?.login}
          >
            <SelectValue>
              {/* 加载中也给字符串：children 为 undefined 时 Radix 会把选项文字
                  portal 进来，之后换成文字会和 React 抢着删节点 */}
              {value === null
                ? labels.follow
                : (selected?.login ?? (data ? missingLabel : ""))}
            </SelectValue>
          </span>
        </SelectTrigger>
        <SelectContent className="w-[var(--radix-select-trigger-width)] max-w-[var(--radix-select-content-available-width)]">
          {accounts.map((account, index) => (
            <React.Fragment key={account.id}>
              <SelectItem
                value={account.id}
                disabled={account.requires_reauth}
                className={itemClass}
              >
                <div className="flex min-w-0 items-center gap-2">
                  <span aria-hidden className="h-4 w-4 shrink-0" />
                  <span
                    className="min-w-0 truncate text-sm font-medium leading-5"
                    title={account.login}
                  >
                    {account.login}
                  </span>
                  {account.requires_reauth && (
                    <span className="ml-1 inline-flex shrink-0 items-center gap-1 text-xs text-warning-text">
                      <AlertTriangle className="h-3 w-3" />
                      {t("provider.form.managedAccount.reauthBadge", {
                        defaultValue: "需要重新登录",
                      })}
                    </span>
                  )}
                </div>
              </SelectItem>
              {(index < accounts.length - 1 || onManageAccounts) && (
                <SelectSeparator className="mx-2 my-0 bg-border/60" />
              )}
            </React.Fragment>
          ))}
          {missing && (
            <SelectItem value={value} className={itemClass}>
              <div className="flex min-w-0 items-center gap-2">
                <span aria-hidden className="h-4 w-4 shrink-0" />
                <span className="min-w-0 truncate text-sm font-medium leading-5 text-warning-text">
                  {missingLabel}
                </span>
              </div>
            </SelectItem>
          )}
          {onManageAccounts && (
            <SelectItem value={MANAGE_ACCOUNTS} className="py-2 pl-6">
              <div className="flex items-center gap-2">
                <Plus className="h-4 w-4 shrink-0 text-fg-2" />
                <span className="truncate text-sm font-medium leading-5">
                  {labels.manage}
                </span>
              </div>
            </SelectItem>
          )}
          {(accounts.length > 0 || missing || onManageAccounts) && (
            <SelectSeparator className="my-1.5 bg-border" />
          )}
          <SelectItem value={DEFAULT_LOGIN} className={itemClass}>
            <div className="flex min-w-0 items-center gap-2">
              <span aria-hidden className="h-4 w-4 shrink-0" />
              <span className="shrink-0 text-sm font-medium leading-5">
                {labels.follow}
              </span>
              <span className="min-w-0 truncate text-sm leading-5 text-fg-2">
                {labels.followDescription}
              </span>
            </div>
          </SelectItem>
        </SelectContent>
      </Select>
      <p className="text-caption text-fg-2">{help}</p>
    </div>
  );
}

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
  const { data, isSuccess } = useManagedAccounts(authProvider);
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
