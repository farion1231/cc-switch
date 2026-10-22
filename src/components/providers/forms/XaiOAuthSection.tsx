import React from "react";
import { useTranslation } from "react-i18next";
import {
  AlertTriangle,
  Check,
  Copy,
  ExternalLink,
  Loader2,
  LogOut,
  Plus,
  Sparkles,
  User,
  X,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { HoverTip } from "@/components/ui/hover-tip";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { copyText } from "@/lib/clipboard";
import { useXaiOauth } from "./hooks/useXaiOauth";
import {
  ManagedAccountRemoveDialog,
  ManagedAccountUsage,
  type ManagedAccountRemoveTarget,
} from "./ManagedAccountRemoveDialog";
import { useManagedAccountUsers } from "./hooks/useManagedAccountUsers";
import {
  ManagedAccountsGroup,
  type GroupAccountRow,
} from "@/components/settings/auth/ManagedAccountsGroup";
import { XaiAccountQuota } from "@/components/settings/auth/AccountQuota";
import { signedInDate } from "@/components/settings/auth/accountDetails";

interface XaiOAuthSectionProps {
  className?: string;
  /**
   * select：供应商表单里（选账号 + 登录）；manage：授权中心里的 xAI 账号区。
   * 不传时按有没有 onAccountSelect 判断（表单都会传）。
   */
  mode?: "manage" | "select";
  selectedAccountId?: string | null;
  onAccountSelect?: (accountId: string | null) => void;
  /** 授权中心里最后一组的「?」向上弹 */
  helpSide?: "top" | "bottom";
}

export const XaiOAuthSection: React.FC<XaiOAuthSectionProps> = ({
  className,
  mode,
  selectedAccountId,
  onAccountSelect,
  helpSide,
}) => {
  const { t, i18n } = useTranslation();
  const [copied, setCopied] = React.useState(false);
  const {
    accounts,
    defaultAccountId,
    hasAnyAccount,
    isAuthenticated,
    isStatusSuccess,
    isStatusError,
    pollingState,
    deviceCode,
    error,
    isPolling,
    isAddingAccount,
    isRemovingAccount,
    isSettingDefaultAccount,
    addAccount,
    removeAccount,
    setDefaultAccount,
    cancelAuth,
    logout,
    refetchStatus,
  } = useXaiOauth();
  const accountUsers = useManagedAccountUsers("xai_oauth", defaultAccountId);
  const [removeTarget, setRemoveTarget] =
    React.useState<ManagedAccountRemoveTarget | null>(null);
  const resolvedMode = mode ?? (onAccountSelect ? "select" : "manage");

  const usableAccounts = accounts.filter((account) => !account.requires_reauth);

  const copyUserCode = async () => {
    if (!deviceCode?.user_code) return;
    await copyText(deviceCode.user_code);
    setCopied(true);
    setTimeout(() => setCopied(false), 2_000);
  };

  // 删账号先确认：确认框里列出在用它的供应商
  const remove = (
    accountId: string,
    login: string,
    event: React.MouseEvent,
  ) => {
    event.preventDefault();
    event.stopPropagation();
    setRemoveTarget({ kind: "one", accountId, login });
  };

  const confirmRemove = () => {
    const target = removeTarget;
    setRemoveTarget(null);
    if (!target) return;
    if (target.kind === "all") {
      logout();
      return;
    }
    removeAccount(target.accountId);
    if (selectedAccountId === target.accountId) onAccountSelect?.(null);
  };

  const removeDialog = (
    <ManagedAccountRemoveDialog
      target={removeTarget}
      serviceName="xAI"
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
      pending={isRemovingAccount}
      onConfirm={confirmRemove}
      onCancel={() => setRemoveTarget(null)}
    />
  );

  if (resolvedMode === "manage") {
    const rows: GroupAccountRow[] = accounts.map((account) => {
      const date = signedInDate(account.authenticated_at, i18n.language);
      const needsReauth = account.requires_reauth;
      return {
        id: account.id,
        login: account.login,
        details: needsReauth
          ? [
              t("authCenter.xaiReauthNote", {
                defaultValue: "登录凭据已失效，用到它的供应商无法使用",
              }),
            ]
          : date
            ? [
                t("authCenter.signedInOn", {
                  defaultValue: "{{date}}登录",
                  date,
                }),
              ]
            : [],
        isDefault: defaultAccountId === account.id,
        needsReauth,
        users: accountUsers([account.id]),
        quota: needsReauth ? undefined : (
          <XaiAccountQuota accountId={account.id} login={account.login} />
        ),
      };
    });

    return (
      <ManagedAccountsGroup
        slug="xai"
        name="xAI"
        iconName="xai"
        help={t("authCenter.group.xaiHelp", {
          defaultValue:
            "用于 Claude Code、Claude Desktop、Codex 的 xAI 预设。没指定账号的供应商用「默认」账号。",
        })}
        helpSide={helpSide}
        accounts={rows}
        status={isStatusError ? "error" : isStatusSuccess ? "ready" : "loading"}
        statusErrorText={t("authCenter.xaiStatusLoadFailed", {
          defaultValue: "无法加载 xAI 账号状态，请重试。",
        })}
        onRetryStatus={() => void refetchStatus()}
        emptyText={t("authCenter.empty", {
          defaultValue: "还没有登录 {{service}} 账号。",
          service: "xAI",
        })}
        loginLabel={t("xaiOauth.login", "使用 xAI 登录")}
        onAdd={addAccount}
        canReauth
        // 后端不支持指定账号重新登录：发起一次普通登录，用同一个 xAI 账号登录会替换掉旧凭据
        onReauth={() => addAccount()}
        onSetDefault={setDefaultAccount}
        settingDefault={isSettingDefaultAccount}
        onRemove={(accountId, login) =>
          setRemoveTarget({ kind: "one", accountId, login })
        }
        onRemoveAll={() =>
          setRemoveTarget({
            kind: "all",
            accountIds: accounts.map((account) => account.id),
          })
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
        }}
      >
        {removeDialog}
      </ManagedAccountsGroup>
    );
  }

  return (
    <div className={`space-y-4 ${className ?? ""}`}>
      <div className="flex items-center justify-between">
        <Label>{t("xaiOauth.authStatus", "xAI OAuth 认证")}</Label>
        <Badge
          variant={isAuthenticated ? "default" : "secondary"}
          className={
            isAuthenticated
              ? "bg-success hover:bg-success"
              : hasAnyAccount
                ? "border-warning text-warning-text"
                : ""
          }
        >
          {isAuthenticated
            ? t("xaiOauth.accountCount", {
                count: usableAccounts.length,
                defaultValue: `${usableAccounts.length} 个可用账号`,
              })
            : hasAnyAccount
              ? t("xaiOauth.reauthRequired", "需要重新登录")
              : t("xaiOauth.notAuthenticated", "未认证")}
        </Badge>
      </div>

      {accounts.length > 0 && onAccountSelect && (
        <div className="space-y-2">
          <Label className="text-sm text-fg-2">
            {t("xaiOauth.selectAccount", "选择账号")}
          </Label>
          <Select
            value={selectedAccountId || "none"}
            onValueChange={(value) =>
              onAccountSelect(value === "none" ? null : value)
            }
          >
            <SelectTrigger>
              <SelectValue
                placeholder={t(
                  "xaiOauth.selectAccountPlaceholder",
                  "选择 xAI 账号",
                )}
              />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="none">
                {t("xaiOauth.useDefaultAccount", "使用默认账号")}
              </SelectItem>
              {accounts.map((account) => (
                <SelectItem
                  key={account.id}
                  value={account.id}
                  disabled={account.requires_reauth}
                >
                  <span className="flex items-center gap-2">
                    {account.requires_reauth ? (
                      <AlertTriangle className="h-4 w-4 text-warning-text" />
                    ) : (
                      <User className="h-4 w-4 text-fg-2" />
                    )}
                    {account.login}
                    {account.requires_reauth && (
                      <span className="text-xs text-warning-text">
                        ({t("xaiOauth.expired", "凭据已失效")})
                      </span>
                    )}
                  </span>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      )}

      {hasAnyAccount && (
        <div className="space-y-2">
          <Label className="text-sm text-fg-2">
            {t("xaiOauth.accounts", "xAI 账号")}
          </Label>
          <div className="space-y-1">
            {accounts.map((account) => (
              <div
                key={account.id}
                className="flex items-center justify-between rounded-md border bg-subtle p-2"
              >
                <div className="flex min-w-0 items-center gap-2">
                  {account.requires_reauth ? (
                    <AlertTriangle className="h-5 w-5 shrink-0 text-warning-text" />
                  ) : (
                    <User className="h-5 w-5 shrink-0 text-fg-2" />
                  )}
                  <span className="truncate text-sm font-medium">
                    {account.login}
                  </span>
                  {defaultAccountId === account.id && (
                    <Badge variant="secondary" className="text-xs">
                      {t("xaiOauth.defaultAccount", "默认")}
                    </Badge>
                  )}
                  {account.requires_reauth && (
                    <Badge
                      variant="outline"
                      className="border-warning text-xs text-warning-text"
                    >
                      {t("xaiOauth.expired", "凭据已失效")}
                    </Badge>
                  )}
                  <ManagedAccountUsage users={accountUsers([account.id])} />
                </div>
                <div className="flex items-center gap-1">
                  {!account.requires_reauth &&
                    defaultAccountId !== account.id && (
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        className="h-7 px-2 text-xs"
                        disabled={isSettingDefaultAccount}
                        onClick={() => setDefaultAccount(account.id)}
                      >
                        {t("xaiOauth.setAsDefault", "设为默认")}
                      </Button>
                    )}
                  <HoverTip content={t("xaiOauth.removeAccount", "移除账号")}>
                    <Button
                      aria-label={t("xaiOauth.removeAccount", "移除账号")}
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7 text-fg-2 hover:text-danger-text"
                      disabled={isRemovingAccount}
                      onClick={(event) =>
                        remove(account.id, account.login, event)
                      }
                    >
                      <X className="h-4 w-4" />
                    </Button>
                  </HoverTip>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {pollingState === "idle" && (
        <Button
          type="button"
          variant="outline"
          className="w-full"
          disabled={isAddingAccount}
          onClick={addAccount}
        >
          {hasAnyAccount ? (
            <Plus className="h-4 w-4" />
          ) : (
            <Sparkles className="h-4 w-4" />
          )}
          {hasAnyAccount
            ? t("xaiOauth.addOrReauth", "添加账号或重新登录")
            : t("xaiOauth.login", "使用 xAI 登录")}
        </Button>
      )}

      {isPolling && deviceCode && (
        <div className="space-y-3 rounded-lg border bg-subtle p-4">
          <div className="flex items-center justify-center gap-2 text-sm text-fg-2">
            <Loader2 className="h-4 w-4 animate-spin" />
            {t("xaiOauth.waitingForAuth", "等待 xAI 授权中…")}
          </div>
          <div className="text-center">
            <p className="mb-1 text-xs text-fg-2">
              {t("xaiOauth.enterCode", "若浏览器未自动填入，请输入：")}
            </p>
            <div className="flex items-center justify-center gap-2">
              <code className="rounded border bg-surface px-4 py-2 font-mono text-2xl font-bold tracking-wider">
                {deviceCode.user_code}
              </code>
              <HoverTip content={t("common.copy")}>
                <Button
                  aria-label={t("common.copy")}
                  type="button"
                  size="icon"
                  variant="ghost"
                  onClick={copyUserCode}
                >
                  {copied ? (
                    <Check className="h-4 w-4 text-success-text" />
                  ) : (
                    <Copy className="h-4 w-4" />
                  )}
                </Button>
              </HoverTip>
            </div>
          </div>
          <div className="text-center">
            <a
              href={deviceCode.verification_uri}
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex items-center gap-1 text-sm text-fg-1 hover:underline"
            >
              {deviceCode.verification_uri}
              <ExternalLink className="h-3 w-3" />
            </a>
          </div>
          <div className="text-center">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={cancelAuth}
            >
              {t("common.cancel", "取消")}
            </Button>
          </div>
        </div>
      )}

      {pollingState === "error" && error && (
        <div className="space-y-2">
          <p className="text-sm text-danger-text">{error}</p>
          <div className="flex gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={addAccount}
            >
              {t("xaiOauth.retry", "重试")}
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={cancelAuth}
            >
              {t("common.cancel", "取消")}
            </Button>
          </div>
        </div>
      )}

      {hasAnyAccount && accounts.length > 1 && (
        <Button
          type="button"
          variant="outline"
          className="w-full"
          onClick={() =>
            setRemoveTarget({
              kind: "all",
              accountIds: accounts.map((account) => account.id),
            })
          }
        >
          <LogOut className="h-4 w-4" />
          {t("xaiOauth.logoutAll", "删除全部 xAI 账号…")}
        </Button>
      )}

      {removeDialog}
    </div>
  );
};

export default XaiOAuthSection;
