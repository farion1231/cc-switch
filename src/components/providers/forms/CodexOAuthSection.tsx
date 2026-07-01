import React from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Loader2,
  LogOut,
  Copy,
  Check,
  ExternalLink,
  Plus,
  X,
  Sparkles,
  User,
  Settings2,
  AlertTriangle,
  RefreshCw,
} from "lucide-react";
import { useCodexOauth } from "./hooks/useCodexOauth";
import { copyText } from "@/lib/clipboard";

interface CodexOAuthSectionProps {
  className?: string;
  /** select 模式只展示账号选择和管理入口；manage 模式展示完整账号管理 */
  mode?: "manage" | "select";
  /** 当前选中的 ChatGPT 账号 ID */
  selectedAccountId?: string | null;
  /** 账号选择回调 */
  onAccountSelect?: (accountId: string | null) => void;
  /** 打开账号管理入口 */
  onManageAccounts?: () => void;
  /** 空选择项文案；默认表示使用托管认证的默认账号 */
  noneOptionLabel?: string;
  /** 是否开启 Codex FAST mode */
  fastModeEnabled?: boolean;
  /** FAST mode 切换回调 */
  onFastModeChange?: (enabled: boolean) => void;
}

/**
 * Codex OAuth 认证区块
 *
 * 通过 OpenAI Device Code 流程登录 ChatGPT Plus/Pro 账号，
 * 用于将 Claude Code 请求反代到 Codex 后端 API。
 */
export const CodexOAuthSection: React.FC<CodexOAuthSectionProps> = ({
  className,
  mode = "manage",
  selectedAccountId,
  onAccountSelect,
  onManageAccounts,
  noneOptionLabel,
  fastModeEnabled = false,
  onFastModeChange,
}) => {
  const { t } = useTranslation();
  const [copied, setCopied] = React.useState(false);

  const {
    accounts,
    defaultAccountId,
    isStatusSuccess,
    hasAnyAccount,
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
  } = useCodexOauth();

  const copyUserCode = async () => {
    if (deviceCode?.user_code) {
      await copyText(deviceCode.user_code);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  };

  const handleAccountSelect = (value: string) => {
    onAccountSelect?.(value === "none" ? null : value);
  };

  React.useEffect(() => {
    // Only clear a bound account when the status query has *successfully*
    // loaded and the account is genuinely gone. On a failed/pending query
    // `accounts` is an empty array, which must not silently unbind the
    // provider's managed account (that would corrupt the saved config).
    if (
      mode !== "select" ||
      !selectedAccountId ||
      !onAccountSelect ||
      !isStatusSuccess
    ) {
      return;
    }

    if (!accounts.some((account) => account.id === selectedAccountId)) {
      onAccountSelect(null);
    }
  }, [accounts, isStatusSuccess, mode, onAccountSelect, selectedAccountId]);

  const handleRemoveAccount = (accountId: string, e: React.MouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
    removeAccount(accountId);
    if (selectedAccountId === accountId) {
      onAccountSelect?.(null);
    }
  };

  // 升级前登录的旧账号没有持久化 id_token，需重新登录补全
  const hasReauthAccounts = accounts.some((account) => account.reauth_required);
  const selectedAccountNeedsReauth =
    !!selectedAccountId &&
    accounts.some(
      (account) => account.id === selectedAccountId && account.reauth_required,
    );

  const accountSelect = onAccountSelect &&
    (mode === "select" || hasAnyAccount || noneOptionLabel) && (
      <div className="space-y-2">
        <Label className="text-sm text-muted-foreground">
          {mode === "select"
            ? t("codexOauth.chatgptAccount", "ChatGPT 账号")
            : t("codexOauth.selectAccount", "选择账号")}
        </Label>
        <Select
          value={selectedAccountId || "none"}
          onValueChange={handleAccountSelect}
        >
          <SelectTrigger>
            <SelectValue
              placeholder={t(
                "codexOauth.selectAccountPlaceholder",
                "选择一个 ChatGPT 账号",
              )}
            />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="none">
              <span className="text-muted-foreground">
                {noneOptionLabel ??
                  t("codexOauth.useDefaultAccount", "使用默认账号")}
              </span>
            </SelectItem>
            {accounts.map((account) => (
              <SelectItem key={account.id} value={account.id}>
                <div className="flex items-center gap-2">
                  <User className="h-4 w-4 text-muted-foreground" />
                  <span>{account.login}</span>
                  {account.reauth_required && (
                    <span className="ml-1 inline-flex items-center gap-1 text-xs text-amber-600 dark:text-amber-400">
                      <AlertTriangle className="h-3 w-3" />
                      {t("codexOauth.reauthBadge", "需要重新登录")}
                    </span>
                  )}
                </div>
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
    );

  return (
    <div className={`space-y-4 ${className || ""}`}>
      {/* 认证状态标题 */}
      {mode === "manage" && (
        <div className="flex items-center justify-between">
          <Label>{t("codexOauth.authStatus", "认证状态")}</Label>
          <Badge
            variant={hasAnyAccount ? "default" : "secondary"}
            className={hasAnyAccount ? "bg-green-500 hover:bg-green-600" : ""}
          >
            {hasAnyAccount
              ? t("codexOauth.accountCount", {
                  count: accounts.length,
                  defaultValue: `${accounts.length} 个账号`,
                })
              : t("codexOauth.notAuthenticated", "未认证")}
          </Badge>
        </div>
      )}

      {/* 旧账号需重新登录提示（缺少 id_token） */}
      {mode === "manage" && hasReauthAccounts && (
        <div className="flex items-start gap-3 rounded-lg border border-amber-300/70 bg-amber-50 p-3 text-amber-900 dark:border-amber-500/40 dark:bg-amber-950/40 dark:text-amber-100">
          <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-amber-500" />
          <div className="space-y-1">
            <p className="text-sm font-medium">
              {t("codexOauth.reauthTitle", "部分账号需要重新登录")}
            </p>
            <p className="text-xs leading-relaxed text-amber-800/90 dark:text-amber-200/80">
              {t(
                "codexOauth.reauthDescription",
                "为与浏览器登录行为保持一致，这些账号需要重新登录以补全所需的登录凭据（id_token）。重新登录后即可正常用于托管绑定。",
              )}
            </p>
          </div>
        </div>
      )}

      {/* 账号选择器 */}
      {mode === "select" && accountSelect ? (
        <div className="flex flex-col gap-2 sm:flex-row sm:items-end">
          <div className="min-w-0 flex-1">{accountSelect}</div>
          {onManageAccounts && (
            <Button
              type="button"
              variant="outline"
              onClick={onManageAccounts}
              className="h-9 shrink-0"
            >
              <Settings2 className="h-4 w-4" />
              {t("codexOauth.manageAccounts", "管理账号")}
            </Button>
          )}
        </div>
      ) : (
        accountSelect
      )}

      {/* select 模式：所选账号需重新登录的内联提示 */}
      {mode === "select" && selectedAccountNeedsReauth && (
        <div className="flex items-start gap-2 rounded-md border border-amber-300/70 bg-amber-50 px-3 py-2 text-xs text-amber-900 dark:border-amber-500/40 dark:bg-amber-950/40 dark:text-amber-100">
          <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0 text-amber-500" />
          <div className="flex-1 leading-relaxed">
            {t(
              "codexOauth.reauthSelectHint",
              "该账号需重新登录以启用托管绑定。",
            )}
            {onManageAccounts && (
              <button
                type="button"
                onClick={onManageAccounts}
                className="ml-1 font-medium underline underline-offset-2 hover:text-amber-700 dark:hover:text-amber-100"
              >
                {t("codexOauth.reauthNow", "立即重新登录")}
              </button>
            )}
          </div>
        </div>
      )}

      {onFastModeChange && (
        <div className="flex items-center justify-between rounded-md border bg-muted/30 p-3">
          <div className="space-y-1 pr-4">
            <Label className="text-sm font-medium">
              {t("codexOauth.fastMode", "FAST mode")}
            </Label>
            <p className="text-xs text-muted-foreground">
              {t("codexOauth.fastModeDescription", {
                defaultValue:
                  'Send service_tier="priority" for lower latency. Turn it off if the ChatGPT Codex backend rejects the parameter.',
              })}
            </p>
          </div>
          <Switch
            checked={fastModeEnabled}
            onCheckedChange={onFastModeChange}
            aria-label={t("codexOauth.fastMode", "FAST mode")}
          />
        </div>
      )}

      {/* 已登录账号列表 */}
      {mode === "manage" && hasAnyAccount && (
        <div className="space-y-2">
          <Label className="text-sm text-muted-foreground">
            {t("codexOauth.loggedInAccounts", "已登录账号")}
          </Label>
          <div className="space-y-1">
            {accounts.map((account) => (
              <div
                key={account.id}
                className={`flex items-center justify-between gap-2 p-2 rounded-md border ${
                  account.reauth_required
                    ? "border-amber-300/70 bg-amber-50/70 dark:border-amber-500/40 dark:bg-amber-950/30"
                    : "bg-muted/30"
                }`}
              >
                <div className="flex min-w-0 items-center gap-2">
                  <User className="h-5 w-5 shrink-0 text-muted-foreground" />
                  <span className="truncate text-sm font-medium">
                    {account.login}
                  </span>
                  {defaultAccountId === account.id && (
                    <Badge variant="secondary" className="shrink-0 text-xs">
                      {t("codexOauth.defaultAccount", "默认")}
                    </Badge>
                  )}
                  {selectedAccountId === account.id && (
                    <Badge variant="outline" className="shrink-0 text-xs">
                      {t("codexOauth.selected", "已选中")}
                    </Badge>
                  )}
                  {account.reauth_required && (
                    <Badge
                      variant="outline"
                      className="shrink-0 gap-1 border-amber-400/70 text-xs text-amber-700 dark:border-amber-500/50 dark:text-amber-300"
                    >
                      <AlertTriangle className="h-3 w-3" />
                      {t("codexOauth.reauthBadge", "需要重新登录")}
                    </Badge>
                  )}
                </div>
                <div className="flex shrink-0 items-center gap-1">
                  {account.reauth_required && (
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      className="h-7 gap-1 border-amber-400/70 px-2 text-xs text-amber-700 hover:bg-amber-100 dark:border-amber-500/50 dark:text-amber-300 dark:hover:bg-amber-900/40"
                      onClick={addAccount}
                      disabled={isAddingAccount}
                    >
                      <RefreshCw className="h-3.5 w-3.5" />
                      {t("codexOauth.reauthLogin", "重新登录")}
                    </Button>
                  )}
                  {defaultAccountId !== account.id && (
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className="h-7 px-2 text-xs text-muted-foreground"
                      onClick={() => setDefaultAccount(account.id)}
                      disabled={isSettingDefaultAccount}
                    >
                      {t("codexOauth.setAsDefault", "设为默认")}
                    </Button>
                  )}
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7 shrink-0 text-muted-foreground hover:text-red-500"
                    onClick={(e) => handleRemoveAccount(account.id, e)}
                    disabled={isRemovingAccount}
                    title={t("codexOauth.removeAccount", "移除账号")}
                  >
                    <X className="h-4 w-4" />
                  </Button>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* 未认证 - 登录按钮 */}
      {mode === "manage" && !hasAnyAccount && pollingState === "idle" && (
        <Button
          type="button"
          onClick={addAccount}
          className="w-full"
          variant="outline"
        >
          <Sparkles className="mr-2 h-4 w-4" />
          {t("codexOauth.loginWithChatGPT", "使用 ChatGPT 登录")}
        </Button>
      )}

      {/* 已有账号 - 添加更多按钮 */}
      {mode === "manage" && hasAnyAccount && pollingState === "idle" && (
        <Button
          type="button"
          onClick={addAccount}
          className="w-full"
          variant="outline"
          disabled={isAddingAccount}
        >
          <Plus className="mr-2 h-4 w-4" />
          {t("codexOauth.addAnotherAccount", "添加其他账号")}
        </Button>
      )}

      {/* 轮询中状态 */}
      {mode === "manage" && isPolling && deviceCode && (
        <div className="space-y-3 p-4 rounded-lg border border-border bg-muted/50">
          <div className="flex items-center justify-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" />
            {t("codexOauth.waitingForAuth", "等待授权中...")}
          </div>

          <div className="text-center">
            <p className="text-xs text-muted-foreground mb-1">
              {t("codexOauth.enterCode", "在浏览器中输入以下代码：")}
            </p>
            <div className="flex items-center justify-center gap-2">
              <code className="text-2xl font-mono font-bold tracking-wider bg-background px-4 py-2 rounded border">
                {deviceCode.user_code}
              </code>
              <Button
                type="button"
                size="icon"
                variant="ghost"
                onClick={copyUserCode}
                title={t("codexOauth.copyCode", "复制代码")}
              >
                {copied ? (
                  <Check className="h-4 w-4 text-green-500" />
                ) : (
                  <Copy className="h-4 w-4" />
                )}
              </Button>
            </div>
          </div>

          <div className="text-center">
            <a
              href={deviceCode.verification_uri}
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex items-center gap-1 text-sm text-blue-500 hover:underline"
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

      {/* 错误状态 */}
      {mode === "manage" && pollingState === "error" && error && (
        <div className="space-y-2">
          <p className="text-sm text-red-500">{error}</p>
          <div className="flex gap-2">
            <Button
              type="button"
              onClick={addAccount}
              variant="outline"
              size="sm"
            >
              {t("codexOauth.retry", "重试")}
            </Button>
            <Button
              type="button"
              onClick={cancelAuth}
              variant="ghost"
              size="sm"
            >
              {t("common.cancel", "取消")}
            </Button>
          </div>
        </div>
      )}

      {/* 注销所有账号 */}
      {mode === "manage" && hasAnyAccount && accounts.length > 1 && (
        <Button
          type="button"
          variant="outline"
          onClick={logout}
          className="w-full text-red-500 hover:text-red-600 hover:bg-red-50 dark:hover:bg-red-950"
        >
          <LogOut className="mr-2 h-4 w-4" />
          {t("codexOauth.logoutAll", "注销所有账号")}
        </Button>
      )}
    </div>
  );
};

export default CodexOAuthSection;
