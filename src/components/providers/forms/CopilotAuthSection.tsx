import React from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { Notice } from "@/components/ui/notice";
import { SegmentedControl } from "@/components/ui/segmented-control";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Loader2,
  User,
  Settings2,
  AlertTriangle,
  CircleAlert,
  RefreshCw,
} from "lucide-react";
import { useCopilotAuth } from "./hooks/useCopilotAuth";
import {
  ManagedAccountRemoveDialog,
  type ManagedAccountRemoveTarget,
} from "./ManagedAccountRemoveDialog";
import { useManagedAccountUsers } from "./hooks/useManagedAccountUsers";
import type { GitHubAccount } from "@/lib/api";
import {
  ManagedAccountsGroup,
  type GroupAccountRow,
} from "@/components/settings/auth/ManagedAccountsGroup";
import { CopilotAccountQuota } from "@/components/settings/auth/AccountQuota";
import { signedInDate } from "@/components/settings/auth/accountDetails";

interface CopilotAuthSectionProps {
  className?: string;
  /** select 模式只展示账号选择和管理入口；manage 模式是授权中心里的账号区 */
  mode?: "manage" | "select";
  /** 当前选中的 GitHub 账号 ID */
  selectedAccountId?: string | null;
  /** 账号选择回调 */
  onAccountSelect?: (accountId: string | null) => void;
  /** 打开账号管理入口 */
  onManageAccounts?: () => void;
  /** 授权中心里最后一组的「?」向上弹 */
  helpSide?: "top" | "bottom";
}

type DeploymentType = "github.com" | "enterprise";

const cleanDomain = (value: string) =>
  value
    .trim()
    .replace(/^https?:\/\//, "")
    .replace(/\/$/, "");

/**
 * Copilot OAuth 认证区块
 *
 * - select：供应商表单里选用哪个 GitHub 账号（+ 管理账号入口）。
 * - manage：授权中心里的 GitHub Copilot 账号区（v7 Auth 画板）。
 */
export const CopilotAuthSection: React.FC<CopilotAuthSectionProps> = ({
  className,
  mode = "manage",
  selectedAccountId,
  onAccountSelect,
  onManageAccounts,
  helpSide,
}) => {
  const { t, i18n } = useTranslation();
  const [deploymentType, setDeploymentType] =
    React.useState<DeploymentType>("github.com");
  const [enterpriseDomain, setEnterpriseDomain] = React.useState("");
  // 「添加账号」先选部署类型；企业域名没填时点登录只提示，不发起
  const [choosing, setChoosing] = React.useState(false);
  const [domainError, setDomainError] = React.useState(false);
  const domainInputRef = React.useRef<HTMLInputElement | null>(null);

  // 根据部署类型计算实际的 GitHub 域名
  const effectiveGithubDomain =
    deploymentType === "enterprise" && enterpriseDomain.trim()
      ? cleanDomain(enterpriseDomain)
      : undefined;

  const {
    accounts,
    defaultAccountId,
    migrationError,
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
  } = useCopilotAuth(effectiveGithubDomain);
  const accountUsers = useManagedAccountUsers(
    "github_copilot",
    defaultAccountId,
  );
  const [removeTarget, setRemoveTarget] =
    React.useState<ManagedAccountRemoveTarget | null>(null);

  // 处理账号选择
  const handleAccountSelect = (value: string) => {
    onAccountSelect?.(value === "none" ? null : value);
  };

  React.useEffect(() => {
    // Only clear a bound account once the status query has *successfully*
    // loaded and the account is genuinely gone. A failed/pending query yields
    // an empty `accounts` array, which must not silently unbind the provider.
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

  const confirmRemove = () => {
    const target = removeTarget;
    setRemoveTarget(null);
    if (!target) return;
    if (target.kind === "all") {
      logout();
      return;
    }
    removeAccount(target.accountId);
    // 如果移除的是当前选中的账号，清除选择
    if (selectedAccountId === target.accountId) {
      onAccountSelect?.(null);
    }
  };

  const statusErrorBanner = isStatusError && (
    <div
      role="alert"
      className="flex items-center gap-2 rounded-md border border-transparent bg-danger-soft px-3 py-2 text-sm text-danger-text"
    >
      <AlertTriangle className="h-4 w-4 shrink-0" />
      <span className="min-w-0 flex-1">
        {t(
          "copilot.statusLoadFailed",
          "无法加载 GitHub Copilot 账号状态，请重试。",
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
        {t("copilot.retry", "重试")}
      </Button>
    </div>
  );

  if (mode === "select") {
    const accountSelect = isStatusSuccess && onAccountSelect && (
      <div className="space-y-2">
        <Label className="text-sm text-fg-2">
          {t("copilot.githubAccount", "GitHub 账号")}
        </Label>
        <Select
          value={selectedAccountId || "none"}
          onValueChange={handleAccountSelect}
        >
          <SelectTrigger>
            <SelectValue
              placeholder={t(
                "copilot.selectAccountPlaceholder",
                "选择一个 GitHub 账号",
              )}
            />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="none">
              <span className="text-fg-2">
                {t("copilot.useDefaultAccount", "使用默认账号")}
              </span>
            </SelectItem>
            {accounts.map((account) => (
              <SelectItem key={account.id} value={account.id}>
                <div className="flex items-center gap-2">
                  <CopilotAccountAvatar account={account} />
                  <span>{account.login}</span>
                </div>
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
    );

    return (
      <div className={`space-y-4 ${className || ""}`}>
        {statusErrorBanner}
        {!isStatusSuccess && !isStatusError && (
          <div className="flex items-center gap-2 text-sm text-fg-2">
            <Loader2 className="h-4 w-4 animate-spin" />
            {t("copilot.statusLoading", "正在加载...")}
          </div>
        )}
        {accountSelect ? (
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
                {t("copilot.manageAccounts", "管理账号")}
              </Button>
            )}
          </div>
        ) : null}
      </div>
    );
  }

  // ── 授权中心（manage） ──

  const openChooser = () => {
    if (choosing) {
      domainInputRef.current?.focus();
      return;
    }
    // 正在登录时保留上次的部署类型（轮询还在用这个域名）
    if (!isAddingAccount) {
      setDeploymentType("github.com");
      setEnterpriseDomain("");
    }
    setDomainError(false);
    setChoosing(true);
  };

  const startLogin = () => {
    if (deploymentType === "enterprise" && !cleanDomain(enterpriseDomain)) {
      setDomainError(true);
      domainInputRef.current?.focus();
      return;
    }
    setChoosing(false);
    addAccount();
  };

  const rows: GroupAccountRow[] = accounts.map((account) => {
    const domain = account.github_domain || "github.com";
    const enterprise = domain !== "github.com";
    const date = signedInDate(account.authenticated_at, i18n.language);
    return {
      id: account.id,
      login: account.login,
      avatarUrl: account.avatar_url,
      details: [
        domain,
        enterprise
          ? t("authCenter.enterpriseServer", {
              defaultValue: "Enterprise Server",
            })
          : null,
        date
          ? t("authCenter.signedInOn", { defaultValue: "{{date}}登录", date })
          : null,
      ].filter(Boolean),
      isDefault: defaultAccountId === account.id,
      // 后端从不把 Copilot 账号标成需要重新登录（reauth_required 恒 false）
      needsReauth: false,
      users: accountUsers([account.id]),
      quota: (
        <CopilotAccountQuota accountId={account.id} login={account.login} />
      ),
    };
  });

  const chooser = choosing && (
    <div className="mx-3 mb-3 flex flex-col gap-2.5 rounded-[8px] border border-border px-3.5 py-3">
      <div className="flex flex-wrap items-center gap-3">
        <span className="text-caption font-medium text-fg-2">
          {t("copilot.deploymentType", { defaultValue: "GitHub 部署类型" })}
        </span>
        <SegmentedControl<DeploymentType>
          size="sm"
          aria-label={t("copilot.deploymentType", {
            defaultValue: "GitHub 部署类型",
          })}
          value={deploymentType}
          onValueChange={(value) => {
            setDeploymentType(value);
            setDomainError(false);
          }}
          items={[
            {
              value: "github.com",
              label: t("copilot.deploymentGitHubCom", {
                defaultValue: "GitHub.com",
              }),
            },
            {
              value: "enterprise",
              label: t("copilot.deploymentEnterprise", {
                defaultValue: "GitHub Enterprise Server",
              }),
            },
          ]}
        />
      </div>
      {deploymentType === "enterprise" && (
        <div className="flex flex-col gap-1">
          <label className="flex items-center gap-3 text-caption font-medium text-fg-2">
            <span className="w-[88px] shrink-0">
              {t("authCenter.enterpriseDomain", { defaultValue: "企业域名" })}
            </span>
            <Input
              ref={domainInputRef}
              autoFocus
              value={enterpriseDomain}
              placeholder={t("copilot.enterpriseDomainPlaceholder", {
                defaultValue: "例如：company.ghe.com",
              })}
              aria-invalid={domainError}
              aria-describedby={
                domainError ? "auth-ghe-domain-hint" : undefined
              }
              onChange={(event) => {
                setEnterpriseDomain(event.target.value);
                if (domainError && cleanDomain(event.target.value)) {
                  setDomainError(false);
                }
              }}
              onKeyDown={(event) => {
                if (event.key === "Enter") startLogin();
              }}
              className={
                domainError
                  ? "h-8 w-[280px] max-w-full rounded-[8px] border-danger text-body shadow-none"
                  : "h-8 w-[280px] max-w-full rounded-[8px] border-border-strong text-body shadow-none"
              }
            />
          </label>
          {domainError && (
            <span
              id="auth-ghe-domain-hint"
              className="flex items-center gap-1 ps-[100px] text-caption text-danger-text"
            >
              <CircleAlert
                aria-hidden="true"
                strokeWidth={1.5}
                className="h-3.5 w-3.5 shrink-0"
              />
              {t("authCenter.enterpriseDomainRequired", {
                defaultValue: "先填企业域名，例如 company.ghe.com",
              })}
            </span>
          )}
        </div>
      )}
      <div className="flex justify-end gap-2">
        <Button
          type="button"
          variant="neutral"
          size="compact"
          onClick={() => {
            setChoosing(false);
            setDomainError(false);
          }}
        >
          {t("common.cancel", { defaultValue: "取消" })}
        </Button>
        <Button
          type="button"
          variant="solid"
          size="compact"
          onClick={startLogin}
        >
          {t("copilot.loginWithGitHub", "使用 GitHub 登录")}
        </Button>
      </div>
    </div>
  );

  return (
    <ManagedAccountsGroup
      slug="copilot"
      name="GitHub Copilot"
      iconName="githubcopilot"
      help={t("authCenter.group.copilotHelp", {
        defaultValue:
          "用于 Claude Code、Claude Desktop 的 GitHub Copilot 预设。没指定账号的供应商用「默认」账号。",
      })}
      helpSide={helpSide}
      accounts={rows}
      status={isStatusError ? "error" : isStatusSuccess ? "ready" : "loading"}
      statusErrorText={t("copilot.statusLoadFailed", {
        defaultValue: "无法加载 GitHub Copilot 账号状态，请重试。",
      })}
      onRetryStatus={() => void refetchStatus()}
      notice={
        migrationError ? (
          <Notice
            tone="warning"
            title={t("copilot.migrationFailed", {
              error: migrationError,
              defaultValue: `旧认证数据迁移失败：${migrationError}`,
            })}
          />
        ) : undefined
      }
      emptyText={t("authCenter.empty", {
        defaultValue: "还没有登录 {{service}} 账号。",
        service: "GitHub Copilot",
      })}
      loginLabel={t("copilot.loginWithGitHub", "使用 GitHub 登录")}
      onAdd={openChooser}
      chooser={chooser || undefined}
      canReauth={false}
      onReauth={() => undefined}
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
      <ManagedAccountRemoveDialog
        target={removeTarget}
        serviceName="GitHub Copilot"
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

const CopilotAccountAvatar: React.FC<{ account: GitHubAccount }> = ({
  account,
}) => {
  const [failed, setFailed] = React.useState(false);

  if (!account.avatar_url || failed) {
    return <User className="h-5 w-5 text-fg-2" />;
  }

  return (
    <img
      src={account.avatar_url}
      alt={account.login}
      className="h-5 w-5 rounded-full"
      loading="lazy"
      referrerPolicy="no-referrer"
      onError={() => setFailed(true)}
    />
  );
};

export default CopilotAuthSection;
