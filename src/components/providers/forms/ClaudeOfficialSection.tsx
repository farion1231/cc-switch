import React from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { AlertTriangle, Loader2, Plus } from "lucide-react";
import { toast } from "@/lib/toast";
import { authApi } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useManagedAuth } from "./hooks/useManagedAuth";
import { useManagedAccountUsers } from "./hooks/useManagedAccountUsers";
import {
  ManagedAccountRemoveDialog,
  type ManagedAccountRemoveTarget,
} from "./ManagedAccountRemoveDialog";
import {
  ManagedAccountsGroup,
  type GroupAccountRow,
} from "@/components/settings/auth/ManagedAccountsGroup";
import { signedInDate } from "@/components/settings/auth/accountDetails";
import { ClaudeAccountQuota } from "@/components/settings/auth/AccountQuota";
import { claudePlanLabel } from "@/lib/claudePlan";

/** 不在默认登录里的账号，过期前多久开始提示 */
const EXPIRY_NOTICE_MS = 7 * 24 * 60 * 60 * 1000;

const SERVICE_NAME = "Claude";

const LOGIN_SWITCH_KEY = ["claude-login-switch"] as const;

function useClaudeLoginSwitch() {
  return useQuery({
    queryKey: LOGIN_SWITCH_KEY,
    queryFn: () => authApi.getClaudeLoginSwitch(),
    staleTime: 30000,
  });
}

/**
 * 切卡不会换 Claude Code 的登录、只能从卡片「打开终端」用选中账号：设了 Claude 配置目录覆盖时。
 */
function useClaudeAccountTerminalOnly(): boolean {
  const { data } = useClaudeLoginSwitch();
  return data?.active === false;
}

/**
 * 浏览器授权后回不到本机（远程桌面、浏览器拦了本机地址）时的退路：把浏览器地址栏里的
 * 地址，或登录页上显示的代码粘贴进来，交给正在等待的 `claude auth login`。
 */
function ClaudePasteCode({ flowId }: { flowId: string }) {
  const { t } = useTranslation();
  const [open, setOpen] = React.useState(false);
  const [value, setValue] = React.useState("");
  const [submitting, setSubmitting] = React.useState(false);
  const [submitted, setSubmitted] = React.useState(false);

  const submit = async () => {
    const pasted = value.trim();
    if (!pasted || submitting) return;
    setSubmitting(true);
    try {
      await authApi.claudeAccountSubmitPaste(flowId, pasted);
      setSubmitted(true);
      setValue("");
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    } finally {
      setSubmitting(false);
    }
  };

  if (submitted) {
    return (
      <span className="text-caption text-fg-2">
        {t("authCenter.claude.pasteSubmitted", {
          defaultValue: "已提交，正在完成登录…",
        })}
      </span>
    );
  }

  if (!open) {
    return (
      <button
        type="button"
        className="self-start text-caption text-fg-2 underline underline-offset-2 hover:text-fg-1"
        onClick={() => setOpen(true)}
      >
        {t("authCenter.claude.pasteToggle", {
          defaultValue: "授权后没有回到这里？",
        })}
      </button>
    );
  }

  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-caption text-fg-2">
        {t("authCenter.claude.pasteHint", {
          defaultValue:
            "把授权后浏览器地址栏里的地址，或页面上显示的代码粘贴到这里。",
        })}
      </span>
      <div className="flex items-center gap-2">
        <Input
          value={value}
          onChange={(event) => setValue(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              void submit();
            }
          }}
          spellCheck={false}
          autoComplete="off"
          className="h-8 flex-1 font-mono text-caption"
          aria-label={t("authCenter.claude.pasteLabel", {
            defaultValue: "授权代码或地址",
          })}
        />
        <Button
          type="button"
          variant="neutral"
          size="compact"
          disabled={!value.trim() || submitting}
          onClick={() => void submit()}
        >
          {submitting && (
            <Loader2 className="h-3.5 w-3.5 motion-safe:animate-spin" />
          )}
          {t("authCenter.claude.pasteSubmit", { defaultValue: "提交" })}
        </Button>
      </div>
    </div>
  );
}

interface ClaudeOfficialSectionProps {
  helpSide?: "top" | "bottom";
}

/**
 * 授权中心的 Claude 官方账号组。每个账号是 CC Switch 数据目录下的一个独立 Claude 配置目录，
 * 在里面跑 `claude auth login` 登录；用哪个号由官方卡的绑定决定，所以没有「默认」。
 * `~/.claude` 里原来的登录是「默认登录」，不在这里列出，也永远不会被改动。
 */
export const ClaudeOfficialSection: React.FC<ClaudeOfficialSectionProps> = ({
  helpSide,
}) => {
  const { t, i18n } = useTranslation();
  const terminalOnly = useClaudeAccountTerminalOnly();
  const {
    accounts,
    isStatusSuccess,
    isStatusError,
    pollingState,
    deviceCode,
    error,
    isPolling,
    isAddingAccount,
    isRemovingAccount,
    addAccount,
    removeAccount,
    cancelAuth,
    refetchStatus,
  } = useManagedAuth("claude_official");
  const accountUsers = useManagedAccountUsers("claude_official", null);
  const [removeTarget, setRemoveTarget] =
    React.useState<ManagedAccountRemoveTarget | null>(null);

  const confirmRemove = () => {
    const target = removeTarget;
    setRemoveTarget(null);
    // 这组没有「删除全部」，只会是单个账号
    if (target?.kind === "one") removeAccount(target.accountId);
  };

  const now = Date.now();
  const rows: GroupAccountRow[] = accounts.map((account) => {
    const date = signedInDate(account.authenticated_at, i18n.language);
    const expiresAt = account.login_expires_at ?? null;
    // 记下的过期时间可能偏早（账号在自己的目录里被 Claude Code 刷新过），所以只写「可能」
    const expired = expiresAt !== null && expiresAt <= now;
    const expiresSoon =
      expiresAt !== null && !expired && expiresAt - now <= EXPIRY_NOTICE_MS;
    const expiryDate = expiresSoon
      ? signedInDate(Math.floor(expiresAt / 1000), i18n.language)
      : null;
    const details: React.ReactNode[] = [];
    const plan = claudePlanLabel(t, account.plan, account.rate_limit_tier);
    if (plan) details.push(plan);
    if (account.requires_reauth) {
      details.push(
        t("authCenter.claude.reauthNote", {
          defaultValue: "登录已失效，请重新登录",
        }),
      );
    } else if (expired) {
      details.push(
        t("authCenter.claude.loginMaybeExpired", {
          defaultValue: "登录可能已过期，切换前请重新登录",
        }),
      );
    } else {
      if (date) {
        details.push(
          t("authCenter.claude.addedOn", {
            defaultValue: "{{date}}添加",
            date,
          }),
        );
      }
      if (expiryDate) {
        details.push(
          t("authCenter.claude.loginExpiresOn", {
            defaultValue: "登录约在{{date}}过期，之后要重新登录",
            date: expiryDate,
          }),
        );
      }
    }
    const needsReauth = account.requires_reauth || expired;
    return {
      id: account.id,
      login: account.login,
      avatarUrl: account.avatar_url,
      details,
      isDefault: false,
      needsReauth,
      users: accountUsers([account.id]),
      quota: needsReauth ? undefined : (
        <ClaudeAccountQuota accountId={account.id} login={account.login} />
      ),
    };
  });

  return (
    <ManagedAccountsGroup
      slug="claude-official"
      name={SERVICE_NAME}
      iconName="claude"
      help={
        terminalOnly
          ? t("authCenter.group.claudeHelpTerminal", {
              defaultValue:
                "Claude Code 的官方订阅账号（Pro / Max / Team）。设置里覆盖了 Claude 配置目录（例如指向 WSL），切卡不会切换 Claude Code 的登录；在官方卡上选一个账号，再从卡片「打开终端」使用。",
            })
          : t("authCenter.group.claudeHelp", {
              defaultValue:
                "Claude Code 的官方订阅账号（Pro / Max / Team）。在 Claude 官方卡上选一个账号，切到这张卡后，新启动的 claude 都用这个号；想同时用几个号，从卡片「打开终端」。登录令牌只留在本机，不上传、不刷新、不登出。第一次切换会自动收进 Claude Code 现在登录的号；切号后 Remote Control 会断开，提示缓存要重建。",
            })
      }
      helpSide={helpSide}
      accounts={rows}
      status={isStatusError ? "error" : isStatusSuccess ? "ready" : "loading"}
      statusErrorText={t("authCenter.claude.statusLoadFailed", {
        defaultValue: "无法读取 Claude 账号，请重试。",
      })}
      onRetryStatus={() => void refetchStatus()}
      emptyText={t("authCenter.claude.empty", {
        defaultValue:
          "还没有添加 Claude 账号。要在多个订阅账号之间切换时在这里登录。",
      })}
      onAdd={addAccount}
      canReauth
      reauthOnlyWhenExpired
      // 用同一个账号再登录一次就会替换掉原来的登录
      onReauth={() => addAccount()}
      settingDefault={false}
      onRemove={(accountId, login) =>
        setRemoveTarget({ kind: "one", accountId, login })
      }
      removing={isRemovingAccount}
      login={{
        starting: isAddingAccount && !isPolling,
        polling: isPolling,
        pollingState,
        deviceCode,
        error,
        onCancel: cancelAuth,
        onRetry: addAccount,
        extra: deviceCode ? (
          <ClaudePasteCode
            key={deviceCode.device_code}
            flowId={deviceCode.device_code}
          />
        ) : undefined,
      }}
    >
      <ManagedAccountRemoveDialog
        target={removeTarget}
        serviceName={SERVICE_NAME}
        users={
          removeTarget
            ? accountUsers(
                removeTarget.kind === "one"
                  ? [removeTarget.accountId]
                  : removeTarget.accountIds,
              )
            : []
        }
        othersRemain={accounts.length > 1}
        note={t("authCenter.claude.removeNote", {
          defaultValue:
            "只删除 CC Switch 为这个账号保存的登录，不会让它在其他地方退出；Claude Code 现在的登录（~/.claude）不受影响。绑定它的卡片会改用 Claude Code 现在的登录。",
        })}
        pending={isRemovingAccount}
        onConfirm={confirmRemove}
        onCancel={() => setRemoveTarget(null)}
      />
    </ManagedAccountsGroup>
  );
};

export default ClaudeOfficialSection;

const DEFAULT_LOGIN = "__default__";
const MANAGE_ACCOUNTS = "__manage_accounts__";

interface ClaudeAccountSelectProps {
  value: string | null;
  onChange: (accountId: string | null) => void;
  /** 打开授权中心添加或管理账号 */
  onManageAccounts?: () => void;
}

/**
 * Claude 官方卡的登录方式，和 Codex 官方卡同一套写法：托管的账号在上，「添加或管理」打开授权
 * 中心，最下面是跟随 Claude Code 登录（~/.claude，不绑号）。
 */
export function ClaudeAccountSelect({
  value,
  onChange,
  onManageAccounts,
}: ClaudeAccountSelectProps) {
  const { t } = useTranslation();
  const terminalOnly = useClaudeAccountTerminalOnly();
  const { data } = useQuery({
    queryKey: ["managed-auth-status", "claude_official"],
    queryFn: () => authApi.authGetStatus("claude_official"),
    staleTime: 30000,
  });
  const accounts = data?.accounts ?? [];
  const selected = accounts.find((a) => a.id === value);
  // 绑的号不在这台电脑上（别的设备同步来的、或已删掉）时也要显示出来，别悄悄改成跟随登录
  const missing = value !== null && !selected;
  const followLabel = t("provider.form.claudeAccount.default", {
    defaultValue: "跟随 Claude Code 登录",
  });
  const missingLabel = t("provider.form.claudeAccount.missing", {
    defaultValue: "不在这台电脑上的账号",
  });
  const label = t("provider.form.claudeAccount.label", {
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
          onChange(next === DEFAULT_LOGIN ? null : next);
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
              {value === null
                ? followLabel
                : (selected?.login ?? (data ? missingLabel : undefined))}
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
                      {t("provider.form.claudeAccount.reauthBadge", {
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
                  {t("provider.form.claudeAccount.manage", {
                    defaultValue: "添加或管理 Claude 账号",
                  })}
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
                {followLabel}
              </span>
              <span className="min-w-0 truncate text-sm leading-5 text-fg-2">
                {t("provider.form.claudeAccount.defaultDescription", {
                  defaultValue: "账号会随 Claude Code 当前登录变化",
                })}
              </span>
            </div>
          </SelectItem>
        </SelectContent>
      </Select>
      <p className="text-caption text-fg-2">
        {terminalOnly
          ? t("provider.form.claudeAccount.helpTerminal", {
              defaultValue:
                "设置里覆盖了 Claude 配置目录，切到这张卡不会切换 Claude Code 的登录，请从卡片「打开终端」使用选中的账号。",
            })
          : t("provider.form.claudeAccount.help", {
              defaultValue:
                "切到这张卡后，Claude Code 的登录会换成选中的账号，任何地方新启动的 claude 都用它。想同时用几个号，从卡片「打开终端」。",
            })}
      </p>
    </div>
  );
}

function useClaudeAccount(accountId: string) {
  const { data, isSuccess } = useQuery({
    queryKey: ["managed-auth-status", "claude_official"],
    queryFn: () => authApi.authGetStatus("claude_official"),
    staleTime: 30000,
  });
  return {
    isSuccess,
    account: data?.accounts.find((a) => a.id === accountId),
  };
}

/**
 * 绑了账号的 Claude 官方卡副标题（官网地址的位置），和 Codex 官方卡同一套写法：写账号，
 * 有备注写备注，卡名里已经有账号就写「Claude 账号」；登录失效、或切卡不会换号（设了配置
 * 目录覆盖）时跟一句提示。
 */
export function ClaudeBoundAccountLine({
  accountId,
  providerName,
  note,
  showPlan = false,
}: {
  accountId: string;
  providerName: string;
  note?: string;
  /** 卡片开了额度查询时才写套餐 */
  showPlan?: boolean;
}) {
  const { t } = useTranslation();
  const terminalOnly = useClaudeAccountTerminalOnly();
  const { isSuccess, account } = useClaudeAccount(accountId);
  if (!isSuccess) return null;
  if (!account) {
    return (
      <div className="mt-0.5 flex min-w-0 items-center gap-1 text-caption text-warning-text">
        <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
        <span className="min-w-0 truncate">
          {t("provider.form.claudeAccount.missing", {
            defaultValue: "不在这台电脑上的账号",
          })}
        </span>
      </div>
    );
  }
  const expiresAt = account.login_expires_at ?? null;
  const needsReauth =
    account.requires_reauth || (expiresAt !== null && expiresAt <= Date.now());
  const plan = showPlan
    ? claudePlanLabel(t, account.plan, account.rate_limit_tier)
    : null;
  const name = providerName.trim();
  const label =
    note ??
    (name === account.login || name.includes(account.login)
      ? t("provider.form.claudeAccount.claudeAccount", {
          defaultValue: "Claude 账号",
        })
      : account.login);
  return (
    <div className="mt-0.5 flex min-w-0 items-center gap-2 text-caption text-fg-2">
      <span className="min-w-0 truncate" title={note ?? account.login}>
        {label}
      </span>
      {/* 跟在账号后面：「me@example.com · Max 20x」（抵掉一半间距，和「 · 」一样宽） */}
      {plan && <span className="-ms-1 shrink-0">· {plan}</span>}
      {needsReauth ? (
        <span className="inline-flex shrink-0 items-center gap-1 text-warning-text">
          <AlertTriangle className="h-3.5 w-3.5" />
          {t("provider.form.claudeAccount.reauthBadge", {
            defaultValue: "需要重新登录",
          })}
        </span>
      ) : terminalOnly ? (
        <span className="shrink-0">
          {t("provider.form.claudeAccount.terminalHint", {
            defaultValue: "从卡片打开终端使用",
          })}
        </span>
      ) : null}
    </div>
  );
}
