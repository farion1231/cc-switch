import React from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
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
} from "lucide-react";
import { useKiroAuth } from "./hooks/useKiroAuth";
import { copyText } from "@/lib/clipboard";

interface KiroAuthSectionProps {
  className?: string;
  /** 当前选中的 Kiro 账号 ID */
  selectedAccountId?: string | null;
  /** 账号选择回调 */
  onAccountSelect?: (accountId: string | null) => void;
}

/**
 * Kiro 认证区块
 *
 * 通过 AWS Builder ID / IAM Identity Center 设备授权流程登录，
 * 用于将 Claude Code 请求反代到 Kiro (AWS CodeWhisperer/Amazon Q) 运行时。
 */
export const KiroAuthSection: React.FC<KiroAuthSectionProps> = ({
  className,
  selectedAccountId,
  onAccountSelect,
}) => {
  const { t } = useTranslation();
  const [copied, setCopied] = React.useState(false);
  const [startUrl, setStartUrl] = React.useState("");
  const [region, setRegion] = React.useState("us-east-1");

  const {
    accounts,
    defaultAccountId,
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
  } = useKiroAuth();

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

  const handleRemoveAccount = (accountId: string, e: React.MouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
    removeAccount(accountId);
    if (selectedAccountId === accountId) {
      onAccountSelect?.(null);
    }
  };

  const handleLogin = () => {
    addAccount({
      startUrl: startUrl.trim() || undefined,
      region: region.trim() || undefined,
    });
  };

  return (
    <div className={`space-y-4 ${className || ""}`}>
      {/* 认证状态标题 */}
      <div className="flex items-center justify-between">
        <Label>{t("kiro.authStatus", "Kiro 认证状态")}</Label>
        <Badge
          variant={hasAnyAccount ? "default" : "secondary"}
          className={hasAnyAccount ? "bg-green-500 hover:bg-green-600" : ""}
        >
          {hasAnyAccount
            ? t("kiro.accountCount", {
                count: accounts.length,
                defaultValue: `${accounts.length} 个账号`,
              })
            : t("kiro.notAuthenticated", "未认证")}
        </Badge>
      </div>

      {/* 账号选择器 */}
      {hasAnyAccount && onAccountSelect && (
        <div className="space-y-2">
          <Label className="text-sm text-muted-foreground">
            {t("kiro.selectAccount", "选择 Kiro 账号")}
          </Label>
          <Select
            value={selectedAccountId || "none"}
            onValueChange={handleAccountSelect}
          >
            <SelectTrigger>
              <SelectValue
                placeholder={t(
                  "kiro.selectAccountPlaceholder",
                  "选择一个 Kiro 账号",
                )}
              />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="none">
                <span className="text-muted-foreground">
                  {t("kiro.useDefaultAccount", "使用默认账号")}
                </span>
              </SelectItem>
              {accounts.map((account) => (
                <SelectItem key={account.id} value={account.id}>
                  <div className="flex items-center gap-2">
                    <User className="h-4 w-4 text-muted-foreground" />
                    <span>{account.login}</span>
                  </div>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      )}

      {/* 已登录账号列表 */}
      {hasAnyAccount && (
        <div className="space-y-2">
          <Label className="text-sm text-muted-foreground">
            {t("kiro.loggedInAccounts", "已登录账号")}
          </Label>
          <div className="space-y-1">
            {accounts.map((account) => (
              <div
                key={account.id}
                className="flex items-center justify-between p-2 rounded-md border bg-muted/30"
              >
                <div className="flex items-center gap-2">
                  <User className="h-5 w-5 text-muted-foreground" />
                  <span className="text-sm font-medium">{account.login}</span>
                  {defaultAccountId === account.id && (
                    <Badge variant="secondary" className="text-xs">
                      {t("kiro.defaultAccount", "默认")}
                    </Badge>
                  )}
                  {selectedAccountId === account.id && (
                    <Badge variant="outline" className="text-xs">
                      {t("kiro.selected", "已选中")}
                    </Badge>
                  )}
                </div>
                <div className="flex items-center gap-1">
                  {defaultAccountId !== account.id && (
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className="h-7 px-2 text-xs text-muted-foreground"
                      onClick={() => setDefaultAccount(account.id)}
                      disabled={isSettingDefaultAccount}
                    >
                      {t("kiro.setAsDefault", "设为默认")}
                    </Button>
                  )}
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7 text-muted-foreground hover:text-red-500"
                    onClick={(e) => handleRemoveAccount(account.id, e)}
                    disabled={isRemovingAccount}
                    title={t("kiro.removeAccount", "移除账号")}
                  >
                    <X className="h-4 w-4" />
                  </Button>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* 登录参数配置 (轮询状态下隐藏，允许用户输入) */}
      {pollingState === "idle" && (
        <div className="space-y-3 rounded-lg border bg-muted/20 p-3">
          <div className="space-y-1.5">
            <Label htmlFor="kiro-start-url" className="text-xs">
              {t("kiro.startUrl", "SSO Start URL (可选)")}
            </Label>
            <Input
              id="kiro-start-url"
              type="text"
              placeholder="https://view.awsapps.com/start (Builder ID)"
              value={startUrl}
              onChange={(e) => setStartUrl(e.target.value)}
              disabled={isAddingAccount}
            />
            <p className="text-[10px] text-muted-foreground">
              留空默认使用 AWS Builder ID 登录。如使用 IAM Identity Center，请输入对应的 SSO 起始 URL。
            </p>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="kiro-region" className="text-xs">
              {t("kiro.region", "Region (区域)")}
            </Label>
            <Input
              id="kiro-region"
              type="text"
              placeholder="us-east-1"
              value={region}
              onChange={(e) => setRegion(e.target.value)}
              disabled={isAddingAccount}
            />
          </div>
        </div>
      )}

      {/* 未认证/添加账号按钮 */}
      {pollingState === "idle" && (
        <Button
          type="button"
          onClick={handleLogin}
          className="w-full"
          variant="outline"
          disabled={isAddingAccount}
        >
          <Sparkles className="mr-2 h-4 w-4" />
          {hasAnyAccount
            ? t("kiro.addAnotherAccount", "添加其他 Kiro 账号")
            : t("kiro.loginWithAWS", "使用 AWS/Kiro 登录")}
        </Button>
      )}

      {/* 轮询中状态 */}
      {isPolling && deviceCode && (
        <div className="space-y-3 p-4 rounded-lg border border-border bg-muted/50">
          <div className="flex items-center justify-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" />
            {t("kiro.waitingForAuth", "等待 AWS 授权中...")}
          </div>

          <div className="text-center">
            <p className="text-xs text-muted-foreground mb-1">
              {t("kiro.enterCode", "在浏览器中输入以下代码：")}
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
                title={t("kiro.copyCode", "复制代码")}
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
              className="inline-flex items-center gap-1 text-sm text-blue-500 hover:underline break-all"
            >
              {deviceCode.verification_uri}
              <ExternalLink className="h-3 w-3 flex-shrink-0" />
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
      {pollingState === "error" && error && (
        <div className="space-y-2">
          <p className="text-sm text-red-500">{error}</p>
          <div className="flex gap-2">
            <Button
              type="button"
              onClick={handleLogin}
              variant="outline"
              size="sm"
            >
              {t("kiro.retry", "重试")}
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
      {hasAnyAccount && accounts.length > 1 && (
        <Button
          type="button"
          variant="outline"
          onClick={logout}
          className="w-full text-red-500 hover:text-red-600 hover:bg-red-50 dark:hover:bg-red-950"
        >
          <LogOut className="mr-2 h-4 w-4" />
          {t("kiro.logoutAll", "注销所有 Kiro 账号")}
        </Button>
      )}
    </div>
  );
};

export default KiroAuthSection;
