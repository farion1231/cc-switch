import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { AlertTriangle } from "lucide-react";
import { authApi } from "@/lib/api";
import type { AppId, AuthCenterTarget, ManagedAuthProvider } from "@/lib/api";
import { canSwitchAgyAccount } from "@/lib/authBinding";
import { managedAuthStatusQuery } from "@/lib/query/managedAuth";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

const OFFICIAL_AUTH_TARGET_BY_APP: Partial<Record<AppId, AuthCenterTarget>> = {
  claude: "claude_official",
  codex: "codex_oauth",
  grokbuild: "xai_oauth",
  agy: "agy_oauth",
  copilot: "github_copilot",
  workbuddy: "workbuddy_account",
  codebuddy: "codebuddy_oauth",
  kimi: "kimi_oauth",
  dsh: "dsh_account",
};

/**
 * Claude 配置目录在 WSL 里时授权中心不能登录 Claude 账号：官方预设不能跳过去，
 * 只能照旧在表单里保存一张跟随 Claude Code 当前登录的官方卡。
 */
/** 返回 `undefined` 表示状态还没读到。 */
export function useClaudeOfficialAddBlockedByWsl(
  appId: AppId | undefined,
): boolean | undefined {
  const isClaude = appId === "claude";
  // 与 ClaudeOfficialSection 共用同一缓存
  const { data } = useQuery({
    queryKey: ["claude-login-switch"],
    queryFn: () => authApi.getClaudeLoginSwitch(),
    staleTime: 30000,
    enabled: isClaude,
  });
  if (!isClaude) return false;
  return data && data.active === false && data.blocked === "wsl";
}

/**
 * 若某应用选择“官方登录”预设应直接前往授权中心统一配置，返回对应的授权中心分区；否则返回 null。
 */
export function officialAuthTargetForPreset(
  appId: AppId | undefined,
  category: string | undefined,
  claudeBlockedByWsl = false,
): AuthCenterTarget | null {
  if (!appId || category !== "official") {
    return null;
  }
  const target = OFFICIAL_AUTH_TARGET_BY_APP[appId] ?? null;
  if (target === "agy_oauth" && !canSwitchAgyAccount()) {
    return null;
  }
  if (target === "claude_official" && claudeBlockedByWsl) {
    return null;
  }
  return target;
}

export interface OfficialAccountPresetPanelProps {
  authProvider: AuthCenterTarget;
  onManageAuthAccounts?: (target: AuthCenterTarget) => void;
  description?: string;
  buttonLabel?: string;
}

/**
 * 统一的新增官方账号入口：引导到授权中心统一登录/收录，登录后自动在供应商页面显示为独立卡片。
 */
export function OfficialAccountPresetPanel({
  authProvider,
  onManageAuthAccounts,
  description,
  buttonLabel,
}: OfficialAccountPresetPanelProps) {
  const { t } = useTranslation();

  return (
    <div className="space-y-3">
      <p className="text-body text-fg-2">
        {description ??
          t("provider.form.officialManaged.addDescription", {
            defaultValue:
              "官方账号在授权中心统一配置和管理。新增账号后会自动在供应商页面显示为独立卡片，方便随时切换。",
          })}
      </p>
      {onManageAuthAccounts && (
        <Button
          type="button"
          variant="neutral"
          onClick={() => onManageAuthAccounts(authProvider)}
        >
          {buttonLabel ??
            t("provider.form.officialManaged.manageAccounts", {
              defaultValue: "前往授权中心管理账号",
            })}
        </Button>
      )}
    </div>
  );
}

export interface OfficialAccountManagedInfoProps {
  authProvider: ManagedAuthProvider;
  accountId: string | null;
  onManageAuthAccounts?: (target: AuthCenterTarget) => void;
  /**
   * 编辑升级前留下的未绑定官方卡时传入：可以选一次授权中心账号（与自动生成的账号卡合并），
   * 保存后绑定固定，不能再改。
   */
  onBind?: (accountId: string) => void;
}

/**
 * 编辑官方登录卡片时的账号归属说明：不提供“登录方式”或账号切换，账号归属与增删统一由授权中心管理。
 */
export function OfficialAccountManagedInfo({
  authProvider,
  accountId,
  onManageAuthAccounts,
  onBind,
}: OfficialAccountManagedInfoProps) {
  const { t } = useTranslation();
  const normalizedId = accountId?.trim() || null;
  const enabled = Boolean(normalizedId) || Boolean(onBind);
  const statusQuery = useQuery({
    ...managedAuthStatusQuery(authProvider),
    enabled,
  });
  const data = enabled ? statusQuery.data : undefined;
  const isSuccess = enabled && statusQuery.isSuccess;
  const account = normalizedId
    ? data?.accounts.find((item) => item.id === normalizedId)
    : undefined;
  const isMissing = Boolean(normalizedId && isSuccess && !account);

  return (
    <div className="space-y-2.5">
      <Label className="text-sm font-medium text-fg-1">
        {t("provider.form.officialManaged.accountLabel", {
          defaultValue: "官方账号",
        })}
      </Label>
      <div className="space-y-2 rounded-panel border border-border bg-subtle/40 px-3.5 py-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex min-w-0 items-center gap-2">
            {normalizedId ? (
              isMissing ? (
                <span className="inline-flex min-w-0 items-center gap-1.5 text-sm font-medium text-warning-text">
                  <AlertTriangle className="h-4 w-4 shrink-0" />
                  <span className="truncate">
                    {t("provider.form.managedAccount.missing", {
                      defaultValue: "不在这台电脑上的账号",
                    })}
                  </span>
                </span>
              ) : (
                <>
                  <span
                    className="min-w-0 truncate text-sm font-medium text-fg-1"
                    title={account?.login ?? normalizedId}
                  >
                    {account?.login ?? normalizedId}
                  </span>
                  {account?.requires_reauth && (
                    <span className="inline-flex shrink-0 items-center gap-1 text-xs text-warning-text">
                      <AlertTriangle className="h-3.5 w-3.5" />
                      {t("provider.form.managedAccount.reauthBadge", {
                        defaultValue: "需要重新登录",
                      })}
                    </span>
                  )}
                </>
              )
            ) : (
              <span className="text-sm font-medium text-fg-1">
                {t("provider.form.officialManaged.unboundAccount", {
                  defaultValue: "未绑定官方账号",
                })}
              </span>
            )}
          </div>
          {onManageAuthAccounts && (
            <Button
              type="button"
              variant="neutral"
              size="compact"
              onClick={() => onManageAuthAccounts(authProvider)}
            >
              {t("provider.form.officialManaged.openAuthCenter", {
                defaultValue: "前往授权中心",
              })}
            </Button>
          )}
        </div>
        {onBind && (
          <Select value={normalizedId ?? undefined} onValueChange={onBind}>
            <SelectTrigger
              aria-label={t("provider.form.officialManaged.bindOnce", {
                defaultValue: "绑定到授权中心账号",
              })}
            >
              <SelectValue
                placeholder={t("provider.form.officialManaged.bindOnce", {
                  defaultValue: "绑定到授权中心账号",
                })}
              />
            </SelectTrigger>
            <SelectContent>
              {(data?.accounts ?? []).map((item) => (
                <SelectItem
                  key={item.id}
                  value={item.id}
                  disabled={item.requires_reauth}
                >
                  {item.login}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        )}
        <p className="text-caption text-fg-2">
          {onBind
            ? t("provider.form.officialManaged.bindOnceHelp", {
                defaultValue:
                  "这张卡来自升级前，还没绑定账号。可以绑定一次授权中心账号，保存后不能再改；同一账号自动生成的卡片可以随后删除。",
              })
            : normalizedId
              ? t("provider.form.officialManaged.editBoundHelp", {
                  defaultValue:
                    "该卡片固定绑定此官方账号，不支持在卡片内切换账号。如需使用其他账号，请直接切换对应账号卡片或在授权中心管理。",
                })
              : t("provider.form.officialManaged.editUnboundHelp", {
                  defaultValue:
                    "每个官方账号均独立显示为一张卡片，不支持在卡片内切换账号。请前往授权中心登录或管理官方账号。",
                })}
        </p>
      </div>
    </div>
  );
}
