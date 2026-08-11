/** @fileoverview Managed Kimi device-login controls for Claude Code providers. */

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
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import KimiOauthAccountQuota from "@/components/KimiOauthAccountQuota";
import { copyText } from "@/lib/clipboard";
import { useKimiOauth } from "./hooks/useKimiOauth";

interface KimiOAuthSectionProps {
  className?: string;
  showAccountQuota?: boolean;
  selectedAccountId?: string | null;
  onAccountSelect?: (accountId: string | null) => void;
}

export const KimiOAuthSection: React.FC<KimiOAuthSectionProps> = ({
  className,
  showAccountQuota = false,
  selectedAccountId,
  onAccountSelect,
}) => {
  const { t } = useTranslation();
  const [copied, setCopied] = React.useState(false);
  const {
    accounts,
    defaultAccountId,
    hasAnyAccount,
    isAuthenticated,
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
  } = useKimiOauth();

  const usableAccounts = accounts.filter((account) => !account.requires_reauth);

  const copyUserCode = async () => {
    if (!deviceCode?.user_code) return;
    await copyText(deviceCode.user_code);
    setCopied(true);
    setTimeout(() => setCopied(false), 2_000);
  };

  const remove = (accountId: string, event: React.MouseEvent) => {
    event.preventDefault();
    event.stopPropagation();
    removeAccount(accountId);
    if (selectedAccountId === accountId) onAccountSelect?.(null);
  };

  return (
    <div className={`space-y-4 ${className ?? ""}`}>
      <div className="flex items-center justify-between">
        <Label>
          {t("kimiOauth.authStatus", "Kimi Code OAuth Authentication")}
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
            ? t("kimiOauth.accountCount", {
                count: usableAccounts.length,
                defaultValue: `Usable accounts: ${usableAccounts.length}`,
              })
            : hasAnyAccount
              ? t("kimiOauth.reauthRequired", "Sign-in required")
              : t("kimiOauth.notAuthenticated", "Not authenticated")}
        </Badge>
      </div>

      {accounts.length > 0 && onAccountSelect && (
        <div className="space-y-2">
          <Label className="text-sm text-muted-foreground">
            {t("kimiOauth.selectAccount", "Select account")}
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
                  "kimiOauth.selectAccountPlaceholder",
                  "Select a Kimi Code account",
                )}
              />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="none">
                {t("kimiOauth.useDefaultAccount", "Use default account")}
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
                    {account.login}
                    {account.requires_reauth && (
                      <span className="text-xs text-amber-600">
                        ({t("kimiOauth.expired", "Credentials expired")})
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
            {t("kimiOauth.accounts", "Kimi Code accounts")}
          </Label>
          <div className="space-y-1">
            {accounts.map((account) => (
              <div
                key={account.id}
                className="space-y-2 rounded-md border bg-muted/30 p-2"
              >
                <div className="flex items-center justify-between">
                  <div className="flex min-w-0 items-center gap-2">
                    {account.requires_reauth ? (
                      <AlertTriangle className="h-5 w-5 shrink-0 text-amber-500" />
                    ) : (
                      <User className="h-5 w-5 shrink-0 text-muted-foreground" />
                    )}
                    <span className="truncate text-sm font-medium">
                      {account.login}
                    </span>
                    {defaultAccountId === account.id && (
                      <Badge variant="secondary" className="text-xs">
                        {t("kimiOauth.defaultAccount", "Default")}
                      </Badge>
                    )}
                    {account.requires_reauth && (
                      <Badge
                        variant="outline"
                        className="border-amber-500 text-xs text-amber-600"
                      >
                        {t("kimiOauth.expired", "Credentials expired")}
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
                          {t("kimiOauth.setAsDefault", "Set as default")}
                        </Button>
                      )}
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7 text-muted-foreground hover:text-red-500"
                      disabled={isRemovingAccount}
                      onClick={(event) => remove(account.id, event)}
                      title={t("kimiOauth.removeAccount", "Remove account")}
                    >
                      <X className="h-4 w-4 shrink-0" />
                    </Button>
                  </div>
                </div>
                {showAccountQuota && (
                  <KimiOauthAccountQuota accountId={account.id} />
                )}
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
            <Plus className="mr-2 h-4 w-4" />
          ) : (
            <Sparkles className="mr-2 h-4 w-4" />
          )}
          {hasAnyAccount
            ? t("kimiOauth.addOrReauth", "Add account or sign in again")
            : t("kimiOauth.login", "Sign in with Kimi Code")}
        </Button>
      )}

      {isPolling && deviceCode && (
        <div className="space-y-3 rounded-lg border bg-muted/50 p-4">
          <div className="flex items-center justify-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" />
            {t(
              "kimiOauth.waitingForAuth",
              "Waiting for Kimi Code authorization…",
            )}
          </div>
          <div className="text-center">
            <p className="mb-1 text-xs text-muted-foreground">
              {t(
                "kimiOauth.enterCode",
                "If the browser did not fill it in automatically, enter:",
              )}
            </p>
            <div className="flex items-center justify-center gap-2">
              <code className="rounded border bg-background px-4 py-2 font-mono text-2xl font-bold tracking-wider">
                {deviceCode.user_code}
              </code>
              <Button
                type="button"
                size="icon"
                variant="ghost"
                onClick={copyUserCode}
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
              {t("common.cancel", "Cancel")}
            </Button>
          </div>
        </div>
      )}

      {pollingState === "error" && error && (
        <div className="space-y-2">
          <p className="text-sm text-red-500">{error}</p>
          <div className="flex gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={addAccount}
            >
              {t("kimiOauth.retry", "Retry")}
            </Button>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={cancelAuth}
            >
              {t("common.cancel", "Cancel")}
            </Button>
          </div>
        </div>
      )}

      {hasAnyAccount && accounts.length > 1 && (
        <Button
          type="button"
          variant="outline"
          className="w-full text-red-500 hover:text-red-600"
          onClick={logout}
        >
          <LogOut className="mr-2 h-4 w-4" />
          {t("kimiOauth.logoutAll", "Remove all Kimi Code accounts")}
        </Button>
      )}
    </div>
  );
};

export default KimiOAuthSection;
