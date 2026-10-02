import React, { useEffect, useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import {
  CircleAlert,
  Copy,
  ExternalLink,
  Loader2,
  MoreHorizontal,
  Plus,
  RefreshCw,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import { HoverTip } from "@/components/ui/hover-tip";
import { Notice } from "@/components/ui/notice";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { ManagedAccountUsage } from "@/components/providers/forms/ManagedAccountRemoveDialog";
import type { ManagedAccountUser } from "@/lib/managedAccountUsage";
import type { ManagedAuthDeviceCodeResponse } from "@/lib/api";
import { copyText } from "@/lib/clipboard";
import { settingsApi } from "@/lib/api";
import { cn } from "@/lib/utils";
import { ProviderIcon } from "@/components/ProviderIcon";

export interface GroupAccountRow {
  id: string;
  login: string;
  avatarUrl?: string | null;
  /** 第二行的片段，用「·」连起来：域名、登录日期；需要重新登录时是原因 */
  details: React.ReactNode[];
  isDefault: boolean;
  needsReauth: boolean;
  /** 在用这个账号的供应商（第二行末尾写「N 个供应商在用」，悬停列名字） */
  users: ManagedAccountUser[];
  /** 右侧的额度（AccountQuotaColumn）；需要重新登录的账号不传 */
  quota?: React.ReactNode;
}

export interface GroupLoginFlow {
  /** 正在向服务要验证码（还没拿到） */
  starting: boolean;
  /** 已拿到验证码，等用户在浏览器里授权 */
  polling: boolean;
  pollingState: "idle" | "polling" | "success" | "error";
  deviceCode: ManagedAuthDeviceCodeResponse | null;
  error: string | null;
  onCancel: () => void;
  onRetry: () => void;
}

interface ManagedAccountsGroupProps {
  /** DOM id 前缀 */
  slug: string;
  name: string;
  /** ProviderIcon 的图标名（githubcopilot / openai / xai） */
  iconName: string;
  /** 「?」里的说明：这个服务的账号用在哪些预设上、没指定账号时用哪个 */
  help: string;
  helpSide?: "top" | "bottom";
  accounts: GroupAccountRow[];
  status: "loading" | "error" | "ready";
  statusErrorText: string;
  onRetryStatus: () => void;
  /** 标题行下面的提示（Copilot 旧数据迁移失败） */
  notice?: React.ReactNode;
  emptyText: string;
  loginLabel: string;
  /** 「添加账号」和空状态的登录按钮（Copilot 先打开部署类型选择） */
  onAdd: () => void;
  /** Copilot 选部署类型的那块；有它时不显示空状态那行 */
  chooser?: React.ReactNode;
  /** ⋯ 里有没有「重新登录」（Copilot 没有） */
  canReauth: boolean;
  onReauth: (accountId: string) => void;
  onSetDefault: (accountId: string) => void;
  settingDefault: boolean;
  onRemove: (accountId: string, login: string) => void;
  onRemoveAll: () => void;
  removing: boolean;
  login: GroupLoginFlow;
  /** 确认框等 */
  children?: React.ReactNode;
}

const PILL =
  "inline-flex h-[18px] shrink-0 items-center rounded-full px-1.5 text-badge";

/**
 * 授权中心的一个服务（v7 Auth 画板）：标题行（图标、名字、「?」、添加账号、⋯ 删除全部），
 * 每个账号一行（头像、名字 + 默认 / 需要重新登录、第二行、额度、⋯），空状态，
 * 登录等待块。删账号走确认框（列出受影响的供应商），由调用方传进 children。
 */
export function ManagedAccountsGroup({
  slug,
  name,
  iconName,
  help,
  helpSide,
  accounts,
  status,
  statusErrorText,
  onRetryStatus,
  notice,
  emptyText,
  loginLabel,
  onAdd,
  chooser,
  canReauth,
  onReauth,
  onSetDefault,
  settingDefault,
  onRemove,
  onRemoveAll,
  removing,
  login,
  children,
}: ManagedAccountsGroupProps) {
  const { t } = useTranslation();
  const pendingId = `auth-pending-${slug}`;
  // 正在重新登录哪个账号（等待块的标题、成功后的 toast 用）
  const [reauthTarget, setReauthTarget] = useState<string | null>(null);
  const busy = login.starting || login.polling;

  const targetAccount = reauthTarget
    ? accounts.find((account) => account.id === reauthTarget)
    : undefined;

  // 登录成功的 toast：pollingState 走到 success 时报一次
  const prevPolling = useRef(login.pollingState);
  useEffect(() => {
    const prev = prevPolling.current;
    prevPolling.current = login.pollingState;
    if (prev === "success" || login.pollingState !== "success") return;
    toast.success(
      targetAccount
        ? t("authCenter.toast.reauthed", {
            defaultValue: "{{login}} 已重新登录。",
            login: targetAccount.login,
          })
        : t("authCenter.toast.added", {
            defaultValue: "已添加 {{service}} 账号。",
            service: name,
          }),
    );
    setReauthTarget(null);
  }, [login.pollingState, name, t, targetAccount]);

  const add = () => {
    if (busy) return;
    setReauthTarget(null);
    onAdd();
  };

  const reauth = (accountId: string) => {
    if (busy) return;
    setReauthTarget(accountId);
    onReauth(accountId);
  };

  const pendingTitle = targetAccount
    ? t("authCenter.pending.reauth", {
        defaultValue: "重新登录 {{login}} · 等待授权中…",
        login: targetAccount.login,
      })
    : accounts.length > 0
      ? t("authCenter.pending.add", {
          defaultValue: "添加 {{service}} 账号 · 等待授权中…",
          service: name,
        })
      : t("authCenter.pending.login", {
          defaultValue: "登录 {{service}} · 等待授权中…",
          service: name,
        });

  const copyCode = async () => {
    if (!login.deviceCode?.user_code) return;
    try {
      await copyText(login.deviceCode.user_code);
      toast.success(
        t("authCenter.pending.copied", { defaultValue: "验证码已复制。" }),
      );
    } catch (e) {
      console.debug("[AuthCenter] Failed to copy user code:", e);
    }
  };

  const openPage = () => {
    const uri = login.deviceCode?.verification_uri;
    if (uri) void settingsApi.openExternal(uri);
  };

  const ready = status === "ready";
  const showEmpty = ready && accounts.length === 0 && !busy && !chooser;

  return (
    <section
      aria-label={name}
      className="shrink-0 rounded-panel border border-border bg-surface"
    >
      <div className="flex h-[52px] items-center gap-3 pe-2.5 ps-4">
        <span
          aria-hidden="true"
          className="flex h-7 w-7 shrink-0 items-center justify-center text-fg-1"
        >
          <ProviderIcon icon={iconName} name={name} size={20} />
        </span>
        <div className="flex min-w-0 flex-1 items-center gap-0.5">
          <h2 className="m-0 truncate text-strong font-semibold text-fg-1">
            {name}
          </h2>
          <HelpTip
            title={t("authCenter.group.helpTitle", {
              defaultValue: "{{service}} 账号怎么用",
              service: name,
            })}
            side={helpSide}
          >
            {help}
          </HelpTip>
        </div>
        {ready && accounts.length > 0 && (
          <Button
            type="button"
            id={`auth-add-${slug}`}
            variant="neutral"
            size="compact"
            className="gap-1.5 pe-3 ps-2.5"
            aria-disabled={busy || undefined}
            aria-describedby={busy ? pendingId : undefined}
            onClick={add}
          >
            <Plus className="h-3.5 w-3.5" strokeWidth={2} />
            {t("authCenter.addAccount", { defaultValue: "添加账号" })}
          </Button>
        )}
        {ready && accounts.length > 1 && (
          <DropdownMenu>
            <HoverTip content={t("common.more", { defaultValue: "更多" })}>
              <DropdownMenuTrigger asChild>
                <Button
                  type="button"
                  variant="quiet"
                  size="icon-compact"
                  aria-label={t("authCenter.groupMenu", {
                    defaultValue: "{{service}} 的更多操作",
                    service: name,
                  })}
                >
                  <MoreHorizontal className="h-[15px] w-[15px]" />
                </Button>
              </DropdownMenuTrigger>
            </HoverTip>
            <DropdownMenuContent
              align="end"
              className="w-[168px] rounded-panel bg-surface p-1 shadow-v7-md"
            >
              <DropdownMenuItem
                disabled={removing}
                onSelect={onRemoveAll}
                className="h-[30px] rounded-control px-2.5 text-body text-danger-text focus:text-danger-text"
              >
                {t("authCenter.removeAll", { defaultValue: "删除全部账号…" })}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        )}
      </div>

      {notice && <div className="px-3 pb-3">{notice}</div>}

      {status === "loading" && (
        <div className="flex items-center gap-2 border-t border-border py-3.5 pe-3 ps-4 text-caption text-fg-2">
          <Loader2 className="h-3.5 w-3.5 motion-safe:animate-spin" />
          {t("authCenter.statusLoading", { defaultValue: "正在加载账号…" })}
        </div>
      )}

      {status === "error" && (
        <div className="flex items-center gap-3 border-t border-border py-3 pe-3 ps-4">
          <CircleAlert
            aria-hidden="true"
            strokeWidth={1.5}
            className="h-4 w-4 shrink-0 text-danger-text"
          />
          <span className="min-w-0 flex-1 text-caption text-fg-1">
            {statusErrorText}
          </span>
          <Button
            type="button"
            variant="neutral"
            size="compact"
            onClick={onRetryStatus}
          >
            <RefreshCw className="h-3.5 w-3.5" strokeWidth={1.5} />
            {t("authCenter.retry", { defaultValue: "重试" })}
          </Button>
        </div>
      )}

      {ready &&
        accounts.map((account) => (
          <AccountRow
            key={account.id}
            account={account}
            canReauth={canReauth}
            busy={busy}
            pendingId={pendingId}
            settingDefault={settingDefault}
            removing={removing}
            onReauth={reauth}
            onSetDefault={onSetDefault}
            onRemove={onRemove}
          />
        ))}

      {showEmpty && (
        <div className="flex items-center gap-4 border-t border-border py-3.5 pe-3 ps-4">
          <span className="min-w-0 flex-1 text-caption text-fg-2">
            {emptyText}
          </span>
          <Button
            type="button"
            id={`auth-login-${slug}`}
            variant="neutral"
            size="compact"
            className="gap-1.5 pe-3 ps-2.5"
            onClick={add}
          >
            <span
              aria-hidden="true"
              className="flex h-3.5 w-3.5 items-center justify-center"
            >
              <ProviderIcon icon={iconName} name={name} size={14} />
            </span>
            {loginLabel}
          </Button>
        </div>
      )}

      {chooser}

      <div role="status">
        {busy && (
          <div className="mx-3 mb-3 flex flex-col gap-2 rounded-[8px] bg-subtle px-3.5 py-3">
            <div className="flex items-center gap-2">
              <Loader2
                aria-hidden="true"
                className="h-[15px] w-[15px] shrink-0 text-fg-2 motion-safe:animate-spin"
              />
              <span
                id={pendingId}
                className="min-w-0 flex-1 text-body font-medium text-fg-1"
              >
                {pendingTitle}
              </span>
              <Button
                type="button"
                variant="neutral"
                size="compact"
                onClick={login.onCancel}
              >
                {t("common.cancel", { defaultValue: "取消" })}
              </Button>
            </div>
            {login.deviceCode && (
              <>
                <div className="flex flex-wrap items-center gap-2 text-caption text-fg-2">
                  <span>
                    {t("authCenter.pending.enterCode", {
                      defaultValue: "在浏览器中输入验证码：",
                    })}
                  </span>
                  <code className="inline-flex h-7 items-center rounded-control border border-border-strong bg-surface px-2.5 font-mono text-section tracking-[1px] text-fg-1">
                    {login.deviceCode.user_code}
                  </code>
                  <HoverTip
                    content={t("authCenter.pending.copy", {
                      defaultValue: "复制验证码",
                    })}
                  >
                    <Button
                      type="button"
                      variant="quiet"
                      size="icon-compact"
                      aria-label={t("authCenter.pending.copy", {
                        defaultValue: "复制验证码",
                      })}
                      onClick={() => void copyCode()}
                    >
                      <Copy className="h-3.5 w-3.5" strokeWidth={1.5} />
                    </Button>
                  </HoverTip>
                  <HoverTip
                    content={t("authCenter.pending.open", {
                      defaultValue: "打开授权页面",
                    })}
                  >
                    <Button
                      type="button"
                      variant="quiet"
                      size="icon-compact"
                      aria-label={t("authCenter.pending.open", {
                        defaultValue: "打开授权页面",
                      })}
                      onClick={openPage}
                    >
                      <ExternalLink className="h-3.5 w-3.5" strokeWidth={1.5} />
                    </Button>
                  </HoverTip>
                </div>
                <span className="text-caption text-fg-2">
                  {t("authCenter.pending.note", {
                    defaultValue:
                      "已在浏览器打开授权页面，完成后这里会自动更新。注意 {{service}} 的使用条款。",
                    service: name,
                  })}
                </span>
              </>
            )}
          </div>
        )}
        {!busy && login.pollingState === "error" && login.error && (
          <div className="mx-3 mb-3">
            <Notice
              tone="danger"
              title={t("authCenter.loginFailed", {
                defaultValue: "登录没有完成",
              })}
              actions={
                <>
                  <Button
                    type="button"
                    variant="neutral"
                    size="compact"
                    onClick={login.onRetry}
                  >
                    {t("authCenter.retry", { defaultValue: "重试" })}
                  </Button>
                  <Button
                    type="button"
                    variant="quiet"
                    size="compact"
                    onClick={login.onCancel}
                  >
                    {t("common.close", { defaultValue: "关闭" })}
                  </Button>
                </>
              }
            >
              <span className="[overflow-wrap:anywhere]">{login.error}</span>
            </Notice>
          </div>
        )}
      </div>

      {children}
    </section>
  );
}

function AccountAvatar({
  login,
  avatarUrl,
}: {
  login: string;
  avatarUrl?: string | null;
}) {
  const [failed, setFailed] = useState(false);
  return (
    <span
      aria-hidden="true"
      className="flex h-7 w-7 shrink-0 items-center justify-center overflow-hidden rounded-full border border-border bg-subtle text-caption font-semibold text-fg-2"
    >
      {avatarUrl && !failed ? (
        <img
          src={avatarUrl}
          alt=""
          className="h-full w-full object-cover"
          loading="lazy"
          referrerPolicy="no-referrer"
          onError={() => setFailed(true)}
        />
      ) : (
        login.charAt(0).toUpperCase()
      )}
    </span>
  );
}

interface AccountRowProps {
  account: GroupAccountRow;
  canReauth: boolean;
  busy: boolean;
  pendingId: string;
  settingDefault: boolean;
  removing: boolean;
  onReauth: (accountId: string) => void;
  onSetDefault: (accountId: string) => void;
  onRemove: (accountId: string, login: string) => void;
}

function AccountRow({
  account,
  canReauth,
  busy,
  pendingId,
  settingDefault,
  removing,
  onReauth,
  onSetDefault,
  onRemove,
}: AccountRowProps) {
  const { t } = useTranslation();
  const subId = useId();
  const canSetDefault = !account.isDefault && !account.needsReauth;
  const separator = " · ";

  return (
    <div className="flex min-h-[56px] items-center gap-3 border-t border-border py-2 pe-2.5 ps-4">
      <AccountAvatar login={account.login} avatarUrl={account.avatarUrl} />
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="flex min-w-0 items-center gap-1.5">
          <span
            title={account.login}
            className="min-w-0 truncate text-body font-medium text-fg-1"
          >
            {account.login}
          </span>
          {account.isDefault && (
            <span className={cn(PILL, "border border-border-strong text-fg-2")}>
              {t("authCenter.default", { defaultValue: "默认" })}
            </span>
          )}
          {account.needsReauth && (
            <span className={cn(PILL, "bg-danger-soft text-danger-text")}>
              {t("authCenter.needsReauth", { defaultValue: "需要重新登录" })}
            </span>
          )}
        </div>
        <div
          id={subId}
          className="flex min-w-0 items-center text-caption text-fg-2"
        >
          <span className="min-w-0 truncate">
            {account.details.map((part, index) => (
              <React.Fragment key={index}>
                {index > 0 && separator}
                {part}
              </React.Fragment>
            ))}
            {account.details.length > 0 && account.users.length > 0
              ? separator
              : null}
          </span>
          <ManagedAccountUsage users={account.users} />
        </div>
      </div>

      {!account.needsReauth && account.quota}

      {account.needsReauth && canReauth && (
        <Button
          type="button"
          variant="neutral"
          size="compact"
          aria-disabled={busy || undefined}
          aria-describedby={busy ? pendingId : subId}
          onClick={() => onReauth(account.id)}
        >
          {t("authCenter.reauth", { defaultValue: "重新登录" })}
        </Button>
      )}

      <DropdownMenu>
        <HoverTip content={t("common.more", { defaultValue: "更多" })}>
          <DropdownMenuTrigger asChild>
            <Button
              type="button"
              variant="quiet"
              size="icon-compact"
              aria-label={t("authCenter.accountMenu", {
                defaultValue: "{{login}} 的更多操作",
                login: account.login,
              })}
            >
              <MoreHorizontal className="h-[15px] w-[15px]" />
            </Button>
          </DropdownMenuTrigger>
        </HoverTip>
        <DropdownMenuContent
          align="end"
          className="min-w-[148px] rounded-panel bg-surface p-1 shadow-v7-md"
        >
          {canSetDefault && (
            <DropdownMenuItem
              disabled={settingDefault}
              onSelect={() => onSetDefault(account.id)}
              className="h-[30px] rounded-control px-2.5 text-body"
            >
              {t("authCenter.setDefault", { defaultValue: "设为默认" })}
            </DropdownMenuItem>
          )}
          {canReauth && (
            <DropdownMenuItem
              disabled={busy}
              onSelect={() => onReauth(account.id)}
              className="min-h-[30px] flex-col items-start justify-center gap-0 rounded-control px-2.5 text-body data-[disabled]:opacity-100"
            >
              <span className={cn(busy && "text-fg-3")}>
                {t("authCenter.reauth", { defaultValue: "重新登录" })}
              </span>
              {busy && (
                <span className="text-caption text-fg-3">
                  {t("authCenter.busyHint", {
                    defaultValue: "先完成或取消正在进行的登录",
                  })}
                </span>
              )}
            </DropdownMenuItem>
          )}
          <DropdownMenuItem
            disabled={removing}
            onSelect={() => onRemove(account.id, account.login)}
            className="h-[30px] rounded-control px-2.5 text-body text-danger-text focus:text-danger-text"
          >
            {t("authCenter.removeAccount", { defaultValue: "删除账号…" })}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}
