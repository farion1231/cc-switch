import React from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Label } from "@/components/ui/label";
import {
  Loader2,
  Github,
  LogOut,
  Copy,
  Check,
  ExternalLink,
} from "lucide-react";
import { useCopilotAuth } from "./hooks/useCopilotAuth";

interface CopilotAuthSectionProps {
  className?: string;
}

/**
 * Copilot OAuth 认证区块
 *
 * 显示 GitHub Copilot 的认证状态，并提供登录/注销操作。
 */
export const CopilotAuthSection: React.FC<CopilotAuthSectionProps> = ({
  className,
}) => {
  const { t } = useTranslation();
  const [copied, setCopied] = React.useState(false);

  const {
    isAuthenticated,
    username,
    pollingState,
    deviceCode,
    error,
    isPolling,
    startAuth,
    cancelAuth,
    logout,
  } = useCopilotAuth();

  // 复制用户码
  const copyUserCode = async () => {
    if (deviceCode?.user_code) {
      await navigator.clipboard.writeText(deviceCode.user_code);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  };

  return (
    <div className={`space-y-4 ${className || ""}`}>
      {/* 认证状态 */}
      <div className="flex items-center justify-between">
        <Label>{t("copilot.authStatus", "GitHub Copilot 认证")}</Label>
        <Badge
          variant={isAuthenticated ? "default" : "secondary"}
          className={isAuthenticated ? "bg-green-500 hover:bg-green-600" : ""}
        >
          {isAuthenticated
            ? t("copilot.authenticated", {
                username,
                defaultValue: `已认证: ${username}`,
              })
            : t("copilot.notAuthenticated", "未认证")}
        </Badge>
      </div>

      {/* 未认证状态 */}
      {!isAuthenticated && pollingState === "idle" && (
        <Button
          type="button"
          onClick={startAuth}
          className="w-full"
          variant="outline"
        >
          <Github className="mr-2 h-4 w-4" />
          {t("copilot.loginWithGitHub", "使用 GitHub 登录")}
        </Button>
      )}

      {/* 轮询中状态 */}
      {isPolling && deviceCode && (
        <div className="space-y-3 p-4 rounded-lg border border-border bg-muted/50">
          <div className="flex items-center justify-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" />
            {t("copilot.waitingForAuth", "等待授权中...")}
          </div>

          {/* 用户码 */}
          <div className="text-center">
            <p className="text-xs text-muted-foreground mb-1">
              {t("copilot.enterCode", "在浏览器中输入以下代码：")}
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
                title={t("copilot.copyCode", "复制代码")}
              >
                {copied ? (
                  <Check className="h-4 w-4 text-green-500" />
                ) : (
                  <Copy className="h-4 w-4" />
                )}
              </Button>
            </div>
          </div>

          {/* 验证链接 */}
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

          {/* 取消按钮 */}
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
          <Button type="button" onClick={startAuth} variant="outline" size="sm">
            {t("copilot.retry", "重试")}
          </Button>
        </div>
      )}

      {/* 成功状态 */}
      {pollingState === "success" && (
        <div className="p-3 rounded-lg border border-green-500/30 bg-green-500/10">
          <p className="text-sm text-green-600 dark:text-green-400">
            {t("copilot.authSuccess", "GitHub Copilot 认证成功！")}
          </p>
        </div>
      )}

      {/* 已认证状态 */}
      {isAuthenticated && (
        <Button
          type="button"
          variant="outline"
          onClick={logout}
          className="w-full"
        >
          <LogOut className="mr-2 h-4 w-4" />
          {t("copilot.logout", "注销")}
        </Button>
      )}
    </div>
  );
};

export default CopilotAuthSection;
