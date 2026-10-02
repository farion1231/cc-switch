import React from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Loader2, Plus, AlertTriangle, RefreshCw } from "lucide-react";
import { useCodexOauth } from "./hooks/useCodexOauth";
import {
  ManagedAccountRemoveDialog,
  type ManagedAccountRemoveTarget,
} from "./ManagedAccountRemoveDialog";
import { useManagedAccountUsers } from "./hooks/useManagedAccountUsers";
import CodexOauthAccountQuota from "@/components/CodexOauthAccountQuota";
import { cn } from "@/lib/utils";
import {
  ManagedAccountsGroup,
  type GroupAccountRow,
} from "@/components/settings/auth/ManagedAccountsGroup";
import {
  signedInDate,
  withMonoToken,
} from "@/components/settings/auth/accountDetails";

interface CodexOAuthSectionProps {
  className?: string;
  /** select 模式只展示账号选择和管理入口；manage 模式展示完整账号管理 */
  mode?: "manage" | "select";
  /** 是否展示每个账号的订阅额度 */
  showAccountQuota?: boolean;
  /** 当前选中的 ChatGPT 账号 ID */
  selectedAccountId?: string | null;
  /** 账号选择回调 */
  onAccountSelect?: (accountId: string | null) => void;
  /** 用户主动选择了登录方式；自动失效清理不会触发 */
  onSelectionConfirmed?: () => void;
  /** 已选账号自动失效；由父级清除与该选择关联的确认状态 */
  onSelectionInvalidated?: () => void;
  /** 打开账号管理入口 */
  onManageAccounts?: () => void;
  /** 账号选择字段标题；官方供应商可使用“登录方式” */
  selectionLabel?: string;
  /** 空选择项文案；默认表示使用托管认证的默认账号 */
  noneOptionLabel?: string;
  /** 空选择项的补充说明；仅由明确知道其含义的调用方提供 */
  noneOptionDescription?: string;
  /** 是否允许不绑定托管账号 */
  allowUnboundSelection?: boolean;
  /** 不绑定选项不依赖托管账号状态，可在状态加载失败时继续选择 */
  allowUnboundSelectionWithoutStatus?: boolean;
  /** 固定展示原生 Codex 当前登录，不允许改绑 */
  nativeLoginOnly?: boolean;
  /** 新建官方卡时不预选登录方式，要求用户明确选择 */
  requireExplicitSelection?: boolean;
  /** 是否开启 Codex FAST mode */
  fastModeEnabled?: boolean;
  /** FAST mode 切换回调 */
  onFastModeChange?: (enabled: boolean) => void;
  /** 授权中心里最后一组的「?」向上弹 */
  helpSide?: "top" | "bottom";
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
  showAccountQuota = false,
  selectedAccountId,
  onAccountSelect,
  onSelectionConfirmed,
  onSelectionInvalidated,
  onManageAccounts,
  selectionLabel,
  noneOptionLabel,
  noneOptionDescription,
  allowUnboundSelection = true,
  allowUnboundSelectionWithoutStatus = false,
  nativeLoginOnly = false,
  requireExplicitSelection = false,
  fastModeEnabled = false,
  onFastModeChange,
  helpSide,
}) => {
  const { t, i18n } = useTranslation();

  const {
    accounts,
    defaultAccountId,
    isStatusSuccess,
    isStatusError,
    hasAnyAccount,
    pollingState,
    deviceCode,
    error,
    isPolling,
    isAddingAccount,
    isRemovingAccount,
    isSettingDefaultAccount,
    addAccount,
    reauthAccount,
    retryAuth,
    removeAccount,
    setDefaultAccount,
    cancelAuth,
    logout,
    refetchStatus,
  } = useCodexOauth();
  const accountUsers = useManagedAccountUsers("codex_oauth", defaultAccountId);
  const [removeTarget, setRemoveTarget] =
    React.useState<ManagedAccountRemoveTarget | null>(null);

  const handleAccountSelect = (value: string) => {
    if (value === "__manage_accounts__") {
      onManageAccounts?.();
      return;
    }
    onSelectionConfirmed?.();
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
      onSelectionInvalidated?.();
      onAccountSelect(null);
    }
  }, [
    accounts,
    isStatusSuccess,
    mode,
    onAccountSelect,
    onSelectionInvalidated,
    selectedAccountId,
  ]);

  const confirmRemove = () => {
    const target = removeTarget;
    setRemoveTarget(null);
    if (!target) return;
    if (target.kind === "all") {
      logout();
      return;
    }
    removeAccount(target.accountId);
    if (selectedAccountId === target.accountId) {
      onSelectionInvalidated?.();
      onAccountSelect?.(null);
    }
  };

  // 升级前登录的旧账号没有持久化 id_token，需重新登录补全
  const selectedAccountNeedsReauth =
    !!selectedAccountId &&
    accounts.some(
      (account) => account.id === selectedAccountId && account.reauth_required,
    );
  const selectedAccount = accounts.find(
    (account) => account.id === selectedAccountId,
  );
  const accountChoicePlaceholder = t(
    "codexOauth.officialAccountPlaceholder",
    "请选择登录方式",
  );
  const accountSelectValue =
    requireExplicitSelection && !selectedAccountId
      ? "__official_account_required__"
      : (selectedAccountId ??
        (allowUnboundSelection ? "none" : "__managed_account_required__"));
  const isAccountSelectionPlaceholder =
    !selectedAccountId && (requireExplicitSelection || !allowUnboundSelection);
  const accountSelectLabel = isAccountSelectionPlaceholder
    ? accountChoicePlaceholder
    : selectedAccount?.login ||
      (selectedAccountId
        ? isStatusError
          ? t("codex.accountStatusUnavailable", "无法读取账号信息")
          : isStatusSuccess
            ? t("codex.boundAccountUnavailable", "绑定的账号不可用")
            : t("codex.accountLoading", "正在加载账号…")
        : undefined) ||
      (allowUnboundSelection
        ? (noneOptionLabel ?? t("codexOauth.useDefaultAccount", "使用默认账号"))
        : t("codexOauth.selectAccountPlaceholder", "选择一个 ChatGPT 账号"));

  const accountSelect = (isStatusSuccess ||
    (allowUnboundSelection && allowUnboundSelectionWithoutStatus)) &&
    onAccountSelect &&
    (mode === "select" || hasAnyAccount || noneOptionLabel) && (
      <div className="space-y-2.5">
        <Label className="text-sm font-medium text-fg-1">
          {selectionLabel ??
            (mode === "select"
              ? t("codexOauth.accountToUse", "使用的账号")
              : t("codexOauth.selectAccount", "选择账号"))}
        </Label>
        <Select
          value={accountSelectValue}
          onValueChange={handleAccountSelect}
          disabled={nativeLoginOnly}
        >
          <SelectTrigger
            className="h-10 min-w-0 rounded-lg bg-surface px-3 shadow-sm"
            aria-label={
              selectionLabel ?? t("codexOauth.accountToUse", "使用的账号")
            }
          >
            <span
              className={cn(
                "min-w-0 flex-1 truncate text-left text-sm font-medium tracking-tight",
                isAccountSelectionPlaceholder &&
                  "font-normal tracking-normal text-fg-2",
              )}
              title={selectedAccount?.login}
            >
              <SelectValue>{accountSelectLabel}</SelectValue>
            </span>
          </SelectTrigger>
          <SelectContent className="w-[var(--radix-select-trigger-width)] max-w-[var(--radix-select-content-available-width)]">
            {requireExplicitSelection && !selectedAccountId && (
              <SelectItem value="__official_account_required__" disabled>
                <span className="text-fg-2">{accountChoicePlaceholder}</span>
              </SelectItem>
            )}
            {!allowUnboundSelection && !selectedAccountId && (
              <SelectItem value="__managed_account_required__" disabled>
                <span className="text-fg-2">{accountChoicePlaceholder}</span>
              </SelectItem>
            )}
            {!nativeLoginOnly &&
              accounts.map((account, index) => (
                <React.Fragment key={account.id}>
                  <SelectItem
                    value={account.id}
                    className="min-w-0 overflow-hidden py-2 pl-6 [&>span:last-child]:min-w-0 [&>span:last-child]:flex-1 [&>span:last-child]:overflow-hidden"
                  >
                    <div className="flex min-w-0 items-center gap-2">
                      <span aria-hidden className="h-4 w-4 shrink-0" />
                      <span
                        className="min-w-0 truncate text-sm font-medium leading-5"
                        title={account.login}
                      >
                        {account.login}
                      </span>
                      {account.reauth_required && (
                        <span className="ml-1 inline-flex shrink-0 items-center gap-1 text-xs text-warning-text">
                          <AlertTriangle className="h-3 w-3" />
                          {t("codexOauth.reauthBadge", "需要重新登录")}
                        </span>
                      )}
                    </div>
                  </SelectItem>
                  {(index < accounts.length - 1 || onManageAccounts) && (
                    <SelectSeparator
                      data-account-divider="true"
                      className="mx-2 my-0 bg-border/60"
                    />
                  )}
                </React.Fragment>
              ))}
            {!nativeLoginOnly && onManageAccounts && (
              <SelectItem value="__manage_accounts__" className="py-2 pl-6">
                <div className="flex items-center gap-2">
                  <Plus className="h-4 w-4 shrink-0 text-fg-2" />
                  <span className="truncate text-sm font-medium leading-5">
                    {t(
                      "codexOauth.addOrManageAccounts",
                      "添加或管理 ChatGPT 账号…",
                    )}
                  </span>
                </div>
              </SelectItem>
            )}
            {allowUnboundSelection &&
              !nativeLoginOnly &&
              (accounts.length > 0 || onManageAccounts) && (
                <SelectSeparator className="my-1.5 bg-border" />
              )}
            {allowUnboundSelection && (
              <SelectItem
                value="none"
                className="min-w-0 overflow-hidden py-2 pl-6 [&>span:last-child]:min-w-0 [&>span:last-child]:flex-1 [&>span:last-child]:overflow-hidden"
              >
                <div className="flex min-w-0 items-center gap-2">
                  <span aria-hidden className="h-4 w-4 shrink-0" />
                  <span className="shrink-0 text-sm font-medium leading-5">
                    {noneOptionLabel ??
                      t("codexOauth.useDefaultAccount", "使用默认账号")}
                  </span>
                  {noneOptionDescription && (
                    <span className="min-w-0 truncate text-sm leading-5 text-fg-2">
                      {noneOptionDescription}
                    </span>
                  )}
                </div>
              </SelectItem>
            )}
          </SelectContent>
        </Select>
      </div>
    );

  const statusErrorBanner = isStatusError && (
    <div
      role="alert"
      className="flex items-center gap-2 rounded-md border border-transparent bg-danger-soft px-3 py-2 text-sm text-danger-text"
    >
      <AlertTriangle className="h-4 w-4 shrink-0" />
      <span className="min-w-0 flex-1">
        {t(
          "codexOauth.statusLoadFailed",
          "无法加载 ChatGPT 账号状态，请重试。",
        )}
      </span>
      <Button
        type="button"
        variant="outline"
        size="sm"
        className="h-7 shrink-0"
        onClick={() => void refetchStatus()}
      >
        <RefreshCw className="mr-1 h-3.5 w-3.5" />
        {t("codexOauth.retry", "重试")}
      </Button>
    </div>
  );

  if (mode === "select") {
    return (
      <div className={`space-y-4 ${className || ""}`}>
        {statusErrorBanner}

        {!isStatusSuccess && !isStatusError && (
          <div className="flex items-center gap-2 text-sm text-fg-2">
            <Loader2 className="h-4 w-4 animate-spin" />
            {t("codexOauth.statusLoading", "正在加载...")}
          </div>
        )}

        {/* 账号选择器 */}
        {accountSelect}

        {/* 所选账号需重新登录的内联提示 */}
        {selectedAccountNeedsReauth && (
          <div className="flex items-start gap-2 rounded-md border border-transparent bg-warning-soft px-3 py-2 text-xs text-warning-text">
            <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0 text-warning-text" />
            <div className="flex-1 leading-relaxed">
              {t(
                "codexOauth.reauthSelectHint",
                "该账号需重新登录以启用托管绑定。",
              )}
              {onManageAccounts && (
                <button
                  type="button"
                  onClick={onManageAccounts}
                  className="ml-1 font-medium underline underline-offset-2 hover:text-warning-text"
                >
                  {t("codexOauth.reauthNow", "立即重新登录")}
                </button>
              )}
            </div>
          </div>
        )}

        {onFastModeChange && (
          <div className="flex items-center justify-between rounded-md border bg-subtle p-3">
            <div className="space-y-1 pr-4">
              <Label className="text-sm font-medium">
                {t("codexOauth.fastMode", "FAST mode")}
              </Label>
              <p className="text-xs text-fg-2">
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
      </div>
    );
  }

  // ── 授权中心（manage） ──

  const rows: GroupAccountRow[] = accounts.map((account) => {
    const date = signedInDate(account.authenticated_at, i18n.language);
    const needsReauth = !!account.reauth_required;
    return {
      id: account.id,
      login: account.login,
      details: needsReauth
        ? [
            withMonoToken(
              t("authCenter.codexReauthNote", {
                defaultValue:
                  "缺少登录凭据（id_token），不能用于 Codex 托管绑定",
              }),
              "id_token",
            ),
          ]
        : date
          ? [t("authCenter.signedInOn", { defaultValue: "{{date}}登录", date })]
          : [],
      isDefault: defaultAccountId === account.id,
      needsReauth,
      users: accountUsers([account.id]),
      quota:
        showAccountQuota && !needsReauth ? (
          <CodexOauthAccountQuota
            accountId={account.id}
            login={account.login}
          />
        ) : undefined,
    };
  });

  return (
    <ManagedAccountsGroup
      slug="chatgpt"
      name="ChatGPT"
      iconName="openai"
      help={t("authCenter.group.chatgptHelp", {
        defaultValue:
          "用于 Claude Code、Claude Desktop 的 ChatGPT 预设，以及 Codex 官方登录绑定。没指定账号的供应商用「默认」账号。",
      })}
      helpSide={helpSide}
      accounts={rows}
      status={isStatusError ? "error" : isStatusSuccess ? "ready" : "loading"}
      statusErrorText={t("codexOauth.statusLoadFailed", {
        defaultValue: "无法加载 ChatGPT 账号状态，请重试。",
      })}
      onRetryStatus={() => void refetchStatus()}
      emptyText={t("authCenter.empty", {
        defaultValue: "还没有登录 {{service}} 账号。",
        service: "ChatGPT",
      })}
      loginLabel={t("codexOauth.loginWithChatGPT", "使用 ChatGPT 登录")}
      onAdd={addAccount}
      canReauth
      onReauth={reauthAccount}
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
        onRetry: retryAuth,
      }}
    >
      <ManagedAccountRemoveDialog
        target={removeTarget}
        serviceName="ChatGPT"
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
    </ManagedAccountsGroup>
  );
};

export default CodexOAuthSection;
