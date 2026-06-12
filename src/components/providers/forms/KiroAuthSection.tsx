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
  X,
  Sparkles,
  User,
  AlertCircle,
  Database,
  Key,
} from "lucide-react";
import { useKiroAuth } from "./hooks/useKiroAuth";
import { RegionCombobox } from "./RegionCombobox";
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
 * 用于将 Claude Code 请求反代到 Kiro 运行时。
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
  const [showIdcConfig, setShowIdcConfig] = React.useState(false);
  const [apiKey, setApiKey] = React.useState("");
  const [showApiKeyInput, setShowApiKeyInput] = React.useState(false);

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
    socialLogin,
    isSocialLoggingIn,
    socialError,
    importDynamic,
    isImporting,
    importError,
    apiKeyLogin,
    isApiKeyLoggingIn,
    apiKeyError,
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

      {/* 登录方式按钮 */}
      {pollingState === "idle" && (
        <div className="space-y-2">
          {/* 1. Google / GitHub 网页登录 */}
          <Button
            type="button"
            onClick={() => socialLogin()}
            className="w-full"
            variant="outline"
            disabled={isAddingAccount || isSocialLoggingIn}
          >
            {isSocialLoggingIn ? (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            ) : (
              <ExternalLink className="mr-2 h-4 w-4" />
            )}
            {t("kiro.loginWithSocial", "使用 Google / GitHub 网页登录")}
          </Button>
          {isSocialLoggingIn && (
            <p className="text-[10px] text-muted-foreground flex items-center gap-1 px-1">
              <AlertCircle className="h-3 w-3 flex-shrink-0" />
              {t(
                "kiro.socialLoginHint",
                "请在打开的浏览器中完成登录（本地回调端口 3128）",
              )}
            </p>
          )}
          {socialError && (
            <p className="text-xs text-red-500 px-1">{socialError}</p>
          )}

          {/* 2. AWS Builder ID */}
          <Button
            type="button"
            onClick={() => addAccount({ region: region.trim() || undefined })}
            className="w-full"
            variant="outline"
            disabled={isAddingAccount}
          >
            <Sparkles className="mr-2 h-4 w-4" />
            {hasAnyAccount
              ? t("kiro.addAnotherBuilderId", "添加 AWS Builder ID 账号")
              : t("kiro.loginWithBuilderId", "使用 AWS Builder ID 登录")}
          </Button>

          {/* 3. IAM Identity Center */}
          <div>
            <Button
              type="button"
              onClick={() => setShowIdcConfig(!showIdcConfig)}
              className="w-full"
              variant="outline"
              disabled={isAddingAccount}
            >
              <ExternalLink className="mr-2 h-4 w-4" />
              {t("kiro.loginWithIdc", "使用 IAM Identity Center 登录")}
            </Button>
            {showIdcConfig && (
              <div className="mt-2 space-y-2 rounded-lg border bg-muted/20 p-3">
                <div className="space-y-1.5">
                  <Label htmlFor="kiro-start-url" className="text-xs">
                    {t("kiro.startUrl", "IAM Start URL")}
                  </Label>
                  <Input
                    id="kiro-start-url"
                    type="text"
                    placeholder="https://your-company.awsapps.com/start"
                    value={startUrl}
                    onChange={(e) => setStartUrl(e.target.value)}
                    disabled={isAddingAccount}
                  />
                </div>
                <div className="space-y-1.5">
                  <Label htmlFor="kiro-region" className="text-xs">
                    {t("kiro.region", "Region (区域)")}
                  </Label>
                  <RegionCombobox
                    id="kiro-region"
                    value={region}
                    onChange={setRegion}
                    disabled={isAddingAccount}
                  />
                </div>
                <Button
                  type="button"
                  onClick={() =>
                    addAccount({
                      startUrl: startUrl.trim(),
                      region: region.trim() || undefined,
                    })
                  }
                  className="w-full"
                  disabled={isAddingAccount || !startUrl.trim()}
                >
                  {t(
                    "kiro.confirmIdcLogin",
                    "确认使用 IAM Identity Center 登录",
                  )}
                </Button>
              </div>
            )}
          </div>

          {/* 4. KIRO_API_KEY (ksk_) 直接接入 */}
          <div>
            <Button
              type="button"
              onClick={() => setShowApiKeyInput(!showApiKeyInput)}
              className="w-full"
              variant="outline"
              disabled={isAddingAccount}
            >
              <Key className="mr-2 h-4 w-4" />
              {t("kiro.loginWithApiKey", "使用 Kiro API Key 登录")}
            </Button>
            {showApiKeyInput && (
              <div className="mt-2 space-y-2 rounded-lg border bg-muted/20 p-3">
                <div className="space-y-1.5">
                  <Label htmlFor="kiro-api-key" className="text-xs">
                    {t("kiro.apiKey", "Kiro API Key")}
                  </Label>
                  <Input
                    id="kiro-api-key"
                    type="password"
                    placeholder="ksk_..."
                    value={apiKey}
                    onChange={(e) => setApiKey(e.target.value)}
                    disabled={isApiKeyLoggingIn}
                  />
                </div>
                <Button
                  type="button"
                  onClick={async () => {
                    try {
                      await apiKeyLogin(apiKey.trim());
                      setApiKey("");
                      setShowApiKeyInput(false);
                    } catch {
                      // 错误已由 hook 记录到 apiKeyError
                    }
                  }}
                  className="w-full"
                  disabled={isApiKeyLoggingIn || !apiKey.trim()}
                >
                  {isApiKeyLoggingIn && (
                    <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  )}
                  {t("kiro.confirmApiKeyLogin", "确认使用 API Key 登录")}
                </Button>
                {apiKeyError && (
                  <p className="text-xs text-red-500 px-1">{apiKeyError}</p>
                )}
                <p className="text-[10px] text-muted-foreground flex items-center gap-1 px-1">
                  <AlertCircle className="h-3 w-3 flex-shrink-0" />
                  {t("kiro.apiKeyHint", "输入以 ksk_ 开头的 Kiro API Key。")}
                </p>
              </div>
            )}
          </div>

          {/* 5. kiro-cli / Kiro IDE 凭证导入（仅点击时读取） */}
          <Button
            type="button"
            onClick={() => importDynamic()}
            className="w-full"
            variant="outline"
            disabled={isAddingAccount || isImporting}
          >
            {isImporting ? (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            ) : (
              <Database className="mr-2 h-4 w-4" />
            )}
            {t("kiro.importLocalCreds", "从 kiro-cli / Kiro IDE 导入凭证")}
          </Button>
          {importError && (
            <p className="text-xs text-red-500 px-1">{importError}</p>
          )}
          <p className="text-[10px] text-muted-foreground flex items-center gap-1 px-1">
            <AlertCircle className="h-3 w-3 flex-shrink-0" />
            {t(
              "kiro.importLocalCredsHint",
              "点击从本地 kiro-cli SQLite / Kiro IDE 缓存读取并导入凭证。",
            )}
          </p>
        </div>
      )}

      {/* 轮询中状态 */}
      {isPolling && deviceCode && (
        <div className="space-y-3 p-4 rounded-lg border border-border bg-muted/50">
          <div className="flex items-center justify-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" />
            {t("kiro.waitingForAuth", "等待 Kiro 授权中...")}
          </div>

          <div className="text-center">
            <p className="text-xs text-muted-foreground mb-1">
              {t("kiro.enterCodeKiro", "在浏览器中核对以下代码：")}
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
