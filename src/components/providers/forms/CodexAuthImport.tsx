import { useId, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Upload, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter,
} from "@/components/ui/dialog";
import { toast } from "@/lib/toast";
import {
  authImportCodexJson,
  CODEX_OAUTH_DUPLICATE_ACCOUNT_ERROR,
} from "@/lib/api/auth";

export function CodexAuthImport({
  disabled,
  onImported,
}: {
  disabled: boolean;
  onImported: () => Promise<unknown>;
}) {
  const { t } = useTranslation();
  const input = useRef<HTMLInputElement>(null);
  // Block repeated events before React commits the disabled state.
  const pending = useRef(false);
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState(false);
  const [content, setContent] = useState("");
  const [error, setError] = useState<string | null>(null);
  const contentId = useId();

  function changeOpen(value: boolean) {
    if (pending.current) return;
    setOpen(value);
    if (!value) {
      setContent("");
      setError(null);
    }
  }

  async function importFile(file: File) {
    if (file.size > 1024 * 1024) {
      toast.error(t("codexOauth.importTooLarge"));
      return;
    }
    await importContent(() => file.text());
  }

  async function importContent(read: () => Promise<string>) {
    if (disabled || pending.current) return;
    pending.current = true;
    setBusy(true);
    setError(null);
    try {
      const text = await read();
      if (new TextEncoder().encode(text).length > 1024 * 1024) {
        const message = t("codexOauth.importTooLarge");
        setError(message);
        toast.error(message);
        return;
      }
      const account = await authImportCodexJson(text);
      setContent("");
      setOpen(false);
      await onImported();
      toast.success(t("codexOauth.importSuccess", { login: account.login }));
    } catch (error) {
      // Parse errors must not echo credentials; never log the submitted JSON.
      const message = String(error);
      const displayError = message.includes(CODEX_OAUTH_DUPLICATE_ACCOUNT_ERROR)
        ? t("codexOauth.duplicateAccount")
        : t("codexOauth.importFailed", { error: message });
      setError(displayError);
      toast.error(displayError);
    } finally {
      pending.current = false;
      setBusy(false);
    }
  }

  return (
    <>
      <input
        ref={input}
        type="file"
        accept=".json,application/json"
        className="hidden"
        aria-label={t("codexOauth.importAuthJson")}
        onChange={(event) => {
          const file = event.target.files?.[0];
          event.target.value = "";
          if (file) void importFile(file);
        }}
      />
      <Button
        type="button"
        variant="outline"
        size="sm"
        className="h-7 gap-1.5"
        disabled={disabled || busy}
        title={t("codexOauth.importHint")}
        onClick={() => changeOpen(true)}
      >
        {busy ? (
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
        ) : (
          <Upload className="h-3.5 w-3.5" />
        )}
        {t("codexOauth.importAuthJson")}
      </Button>
      <Dialog open={open} onOpenChange={changeOpen}>
        <DialogContent className="max-w-xl">
          <form
            onSubmit={(event) => {
              event.preventDefault();
              if (content.trim()) void importContent(async () => content);
            }}
          >
            <DialogHeader>
              <DialogTitle>{t("codexOauth.importAuthJson")}</DialogTitle>
              <DialogDescription>
                {t("codexOauth.pasteDescription")}
              </DialogDescription>
            </DialogHeader>
            <div className="space-y-2 px-6">
              <label htmlFor={contentId} className="text-sm font-medium">
                {t("codexOauth.pasteLabel")}
              </label>
              <Textarea
                id={contentId}
                className="min-h-[220px] max-h-[45vh] resize-y font-mono text-xs"
                value={content}
                onChange={(event) => {
                  setContent(event.target.value);
                  setError(null);
                }}
                placeholder={t("codexOauth.pastePlaceholder")}
                disabled={busy}
                aria-invalid={Boolean(error)}
                aria-describedby={error ? `${contentId}-error` : undefined}
              />
              {error && (
                <p
                  id={`${contentId}-error`}
                  role="alert"
                  className="text-sm text-red-500"
                >
                  {error}
                </p>
              )}
            </div>
            <DialogFooter className="flex-wrap">
              <Button
                type="button"
                variant="outline"
                className="mr-auto"
                disabled={disabled || busy}
                onClick={() => input.current?.click()}
              >
                {t("codexOauth.chooseAuthFile")}
              </Button>
              <Button
                type="button"
                variant="outline"
                disabled={busy}
                onClick={() => changeOpen(false)}
              >
                {t("common.cancel")}
              </Button>
              <Button
                type="submit"
                disabled={disabled || busy || !content.trim()}
              >
                {busy && (
                  <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />
                )}
                {t("codexOauth.importPasted")}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </>
  );
}
