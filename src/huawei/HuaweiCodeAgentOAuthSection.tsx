import React from "react";
import { HuaweiQuotaUsage } from "./HuaweiQuotaUsage";
import { useTranslation } from "react-i18next";
import {
  AlertTriangle,
  Loader2,
  LogOut,
  Plus,
  Sparkles,
  User,
  X,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useHuaweiCodeAgentOauth } from "./hooks/useHuaweiCodeAgentOauth";

interface HuaweiCodeAgentOAuthSectionProps {
  className?: string;
  selectedAccountId?: string | null;
  onAccountSelect?: (accountId: string | null) => void;
}

export const HuaweiCodeAgentOAuthSection: React.FC<
  HuaweiCodeAgentOAuthSectionProps
> = ({ className, selectedAccountId, onAccountSelect }) => {
  const { t } = useTranslation();
  const [user, setUser] = React.useState("");
  const [password, setPassword] = React.useState("");
  const [error, setError] = React.useState<string | null>(null);

  const {
    accounts,
    defaultAccountId,
    hasAnyAccount,
    isAuthenticated,
    addAccount,
    isAddingAccount,
    removeAccount,
    isRemovingAccount,
    setDefaultAccount,
    isSettingDefaultAccount,
    logout,
  } = useHuaweiCodeAgentOauth();

  const handleLogin = async () => {
    if (!user || !password) {
      setError(
        t("huaweiCodeAgent2.emptyCredentials", {
          defaultValue: "请输入用户名和密码",
        }),
      );
      return;
    }
    setError(null);
    try {
      await addAccount({ user, password });
      setPassword("");
      setUser("");
    } catch (e) {
      setError(String(e));
    }
  };

  const handleRemove = (accountId: string, e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    removeAccount(accountId);
    if (selectedAccountId === accountId) onAccountSelect?.(null);
  };

  return (
    <div className={`space-y-4 ${className ?? ""}`}>
      <div className="flex items-center justify-between">
        <Label>
          {t("huaweiCodeAgent2.title", { defaultValue: "华为 CodeAgent 认证" })}
        </Label>
        <Badge
          variant={isAuthenticated ? "default" : "secondary"}
          className={
            isAuthenticated
              ? "bg-green-500 hover:bg-green-600"
              : hasAnyAccount
                ? "border-amber-500 text-amber-600"
                : ""
          }
        >
          {isAuthenticated
            ? t("huaweiCodeAgent2.accountCount", {
                count: accounts.filter((a) => !a.requires_reauth).length,
                defaultValue: `${accounts.filter((a) => !a.requires_reauth).length} 个可用账号`,
              })
            : hasAnyAccount
              ? t("huaweiCodeAgent2.reauthRequired", {
                  defaultValue: "需要重新登录",
                })
              : t("huaweiCodeAgent2.notLoggedIn", { defaultValue: "未认证" })}
        </Badge>
      </div>

      {hasAnyAccount && onAccountSelect && (
        <div className="space-y-2">
          <Label className="text-sm text-muted-foreground">
            {t("huaweiCodeAgent2.selectAccount", { defaultValue: "选择账号" })}
          </Label>
          <Select
            value={selectedAccountId || "none"}
            onValueChange={(value) =>
              onAccountSelect(value === "none" ? null : value)
            }
          >
            <SelectTrigger>
              <SelectValue
                placeholder={t("huaweiCodeAgent2.selectAccountPlaceholder", {
                  defaultValue: "选择华为 CodeAgent 账号",
                })}
              />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="none">
                {t("huaweiCodeAgent2.useDefaultAccount", {
                  defaultValue: "使用默认账号",
                })}
              </SelectItem>
              {accounts.map((account) => (
                <SelectItem
                  key={account.id}
                  value={account.id}
                  disabled={account.requires_reauth}
                >
                  <span className="flex items-center gap-2">
                    {account.requires_reauth ? (
                      <AlertTriangle className="h-4 w-4 text-amber-500" />
                    ) : (
                      <User className="h-4 w-4 text-muted-foreground" />
                    )}
                    {account.user}
                    {account.requires_reauth && (
                      <span className="text-xs text-amber-600">
                        (
                        {t("huaweiCodeAgent2.expired", {
                          defaultValue: "凭据已失效",
                        })}
                        )
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
          <Label className="text-sm text-muted-foreground">
            {t("huaweiCodeAgent2.accounts", {
              defaultValue: "华为 CodeAgent 账号",
            })}
          </Label>
          <div className="space-y-1">
            {accounts.map((account) => (
              <div
                key={account.id}
                className="flex items-center justify-between rounded-md border bg-muted/30 p-2"
              >
                <div className="flex min-w-0 items-center gap-2">
                  {account.requires_reauth ? (
                    <AlertTriangle className="h-5 w-5 shrink-0 text-amber-500" />
                  ) : (
                    <User className="h-5 w-5 shrink-0 text-muted-foreground" />
                  )}
                  <span className="truncate text-sm font-medium">
                    {account.user}
                  </span>
                  {defaultAccountId === account.id && (
                    <Badge variant="secondary" className="text-xs">
                      {t("huaweiCodeAgent2.defaultAccount", {
                        defaultValue: "默认",
                      })}
                    </Badge>
                  )}
                  {account.requires_reauth && (
                    <Badge
                      variant="outline"
                      className="border-amber-500 text-xs text-amber-600"
                    >
                      {t("huaweiCodeAgent2.expired", {
                        defaultValue: "凭据已失效",
                      })}
                    </Badge>
                  )}
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
                        {t("huaweiCodeAgent2.setAsDefault", {
                          defaultValue: "设为默认",
                        })}
                      </Button>
                    )}
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7 text-muted-foreground hover:text-red-500"
                    disabled={isRemovingAccount}
                    onClick={(e) => handleRemove(account.id, e)}
                    title={t("huaweiCodeAgent2.removeAccount", {
                      defaultValue: "移除账号",
                    })}
                  >
                    <X className="h-4 w-4" />
                  </Button>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {hasAnyAccount && (
        <HuaweiQuotaUsage
          accountId={selectedAccountId ?? defaultAccountId ?? null}
        />
      )}

      {hasAnyAccount && (
        <Button
          type="button"
          variant="outline"
          className="w-full text-red-500 hover:text-red-600"
          onClick={() => logout()}
        >
          <LogOut className="mr-2 h-4 w-4" />
          {t("huaweiCodeAgent2.logoutAll", { defaultValue: "移除所有账号" })}
        </Button>
      )}

      <div className="space-y-2">
        <Input
          placeholder={t("huaweiCodeAgent2.username", {
            defaultValue: "用户名",
          })}
          value={user}
          onChange={(e) => setUser(e.target.value)}
        />
        <Input
          type="password"
          placeholder={t("huaweiCodeAgent2.password", { defaultValue: "密码" })}
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") handleLogin();
          }}
        />
        <Button
          type="button"
          onClick={handleLogin}
          className="w-full"
          variant="outline"
          disabled={isAddingAccount}
        >
          {isAddingAccount && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
          {hasAnyAccount ? (
            <Plus className="mr-2 h-4 w-4" />
          ) : (
            <Sparkles className="mr-2 h-4 w-4" />
          )}
          {hasAnyAccount
            ? t("huaweiCodeAgent2.addOrReauth", {
                defaultValue: "添加账号或重新登录",
              })
            : t("huaweiCodeAgent2.login", { defaultValue: "登录" })}
        </Button>
      </div>

      {error && <p className="text-sm text-red-500">{error}</p>}

      <p className="text-xs text-muted-foreground">
        {t("huaweiCodeAgent2.description", {
          defaultValue: "使用华为 CodeAgent 账号认证访问内部 LLM 服务",
        })}
      </p>
    </div>
  );
};

export default HuaweiCodeAgentOAuthSection;
