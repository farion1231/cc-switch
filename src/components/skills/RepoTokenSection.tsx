import { useState } from "react";
import { useTranslation } from "react-i18next";
import { KeyRound, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { HoverTip } from "@/components/ui/hover-tip";
import { skillErrorReason } from "@/lib/errors/skillErrorParser";
import { cn } from "@/lib/utils";
import {
  FieldError,
  LABEL_CLASS,
  MONO_FIELD_CLASS,
} from "@/components/mcp/formBits";
import { tokenHostFromInput } from "./repoUrl";

export interface RepoTokenSectionProps {
  /** 已设置令牌的主机 */
  hosts: string[];
  /** 令牌存在系统凭据存储里；false 时存在本机仅自己可读的文件里 */
  secureStorage: boolean;
  /** 已添加仓库用到的主机，作为「站点」输入框的候选 */
  knownHosts: string[];
  onSave: (host: string, token: string) => Promise<unknown>;
  onRemove: (host: string) => Promise<unknown>;
}

const errorText = (error: unknown) =>
  error instanceof Error ? error.message : String(error);

/**
 * 仓库管理抽屉里的「访问令牌」区（#9666）：按站点保存，读私有仓库时带上。
 * 令牌只写不读，界面上只列出设置过令牌的站点。
 */
export function RepoTokenSection({
  hosts,
  secureStorage,
  knownHosts,
  onSave,
  onRemove,
}: RepoTokenSectionProps) {
  const { t } = useTranslation();
  const [host, setHost] = useState("");
  const [token, setToken] = useState("");
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);
  const suggestions = knownHosts.filter((known) => !hosts.includes(known));

  const handleSave = async () => {
    setError("");
    const normalizedHost = tokenHostFromInput(host);
    if (!normalizedHost || !token.trim()) {
      setError(t("skills.repo.token.missingFields"));
      return;
    }
    setSaving(true);
    try {
      await onSave(normalizedHost, token);
      setHost("");
      setToken("");
    } catch (e) {
      setError(skillErrorReason(errorText(e), t));
    } finally {
      setSaving(false);
    }
  };

  const handleRemove = async (target: string) => {
    setError("");
    try {
      await onRemove(target);
    } catch (e) {
      setError(skillErrorReason(errorText(e), t));
    }
  };

  return (
    <section
      aria-labelledby="sk-repo-token-title"
      className="flex flex-col gap-3"
    >
      <div className="flex flex-col gap-1">
        <h3
          id="sk-repo-token-title"
          className="m-0 text-body font-semibold text-fg-1"
        >
          {t("skills.repo.token.title")}
        </h3>
        <p className="m-0 text-caption text-fg-2">
          {secureStorage
            ? t("skills.repo.token.description")
            : t("skills.repo.token.descriptionFile")}
        </p>
      </div>

      {hosts.length === 0 ? (
        <p className="m-0 text-caption text-fg-2">
          {t("skills.repo.token.empty")}
        </p>
      ) : (
        <ul className="m-0 list-none rounded-panel border border-border p-0">
          {hosts.map((tokenHost, index) => (
            <li
              key={tokenHost}
              className={cn(
                "flex min-h-10 items-center gap-2.5 py-1 pe-2 ps-3.5",
                index > 0 && "border-t border-border",
              )}
            >
              <KeyRound
                aria-hidden="true"
                className="h-4 w-4 shrink-0 text-fg-2"
                strokeWidth={1.5}
              />
              <span className="min-w-0 flex-1 truncate font-mono text-caption text-fg-1">
                {tokenHost}
              </span>
              <HoverTip content={t("common.delete")}>
                <Button
                  type="button"
                  variant="quiet"
                  size="icon-compact"
                  aria-label={t("skills.repo.token.removeAria", {
                    host: tokenHost,
                  })}
                  className="hover:text-danger-text"
                  onClick={() => void handleRemove(tokenHost)}
                >
                  <Trash2 className="h-4 w-4" strokeWidth={1.5} />
                </Button>
              </HoverTip>
            </li>
          ))}
        </ul>
      )}

      <div className="flex flex-col gap-1.5">
        <label htmlFor="sk-repo-token-host" className={LABEL_CLASS}>
          {t("skills.repo.token.host")}
        </label>
        <input
          id="sk-repo-token-host"
          className={MONO_FIELD_CLASS}
          placeholder={t("skills.repo.token.hostPlaceholder")}
          list={suggestions.length > 0 ? "sk-repo-token-hosts" : undefined}
          value={host}
          autoComplete="off"
          spellCheck={false}
          autoCorrect="off"
          autoCapitalize="off"
          onChange={(event) => setHost(event.target.value)}
        />
        {suggestions.length > 0 && (
          <datalist id="sk-repo-token-hosts">
            {suggestions.map((known) => (
              <option key={known} value={known} />
            ))}
          </datalist>
        )}
      </div>
      <div className="flex flex-col gap-1.5">
        <label htmlFor="sk-repo-token-value" className={LABEL_CLASS}>
          {t("skills.repo.token.value")}
        </label>
        <input
          id="sk-repo-token-value"
          type="password"
          className={MONO_FIELD_CLASS}
          placeholder={t("skills.repo.token.valuePlaceholder")}
          value={token}
          aria-invalid={Boolean(error)}
          aria-describedby={error ? "sk-repo-token-error" : undefined}
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => setToken(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") void handleSave();
          }}
        />
      </div>
      {error && <FieldError id="sk-repo-token-error">{error}</FieldError>}
      <Button
        type="button"
        variant="neutral"
        size="regular"
        className="self-start"
        disabled={saving}
        onClick={() => void handleSave()}
      >
        <KeyRound className="h-4 w-4" strokeWidth={2} />
        {t("skills.repo.token.save")}
      </Button>
    </section>
  );
}
