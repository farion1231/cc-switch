import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ProviderIcon } from "@/components/ProviderIcon";

interface McodeStatus {
  source: "token_plan" | "minimax_api_key";
  hasApiKey: boolean;
}

type Action = "status" | "saveApiKey" | "useApiKey" | "useTokenPlan" | "test";

export function McodeSettings() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<McodeStatus>();
  const [apiKey, setApiKey] = useState("");
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");

  const run = useCallback(async (action: Action, key?: string) => {
    setBusy(true);
    setError("");
    setMessage("");
    try {
      const next = await invoke<McodeStatus>("manage_mcode", {
        action,
        apiKey: key ?? null,
      });
      setStatus(next);
      if (action === "saveApiKey") setApiKey("");
      if (action !== "status") {
        setMessage(action === "test" ? "mcode.testPassed" : "mcode.saved");
      }
    } catch (error) {
      setError(String(error));
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    void run("status");
  }, [run]);

  return (
    <section
      className="rounded-xl glass-card p-6 space-y-4"
      aria-labelledby="mcode-title"
    >
      <div className="flex items-center gap-3">
        <ProviderIcon icon="minimax" name="MCode" size={24} />
        <h3 id="mcode-title" className="text-base font-semibold">
          MCode
        </h3>
      </div>
      <p className="text-sm text-muted-foreground">{t("mcode.description")}</p>
      {status && (
        <p className="text-sm">
          {t("mcode.source")}:{" "}
          {status.source === "minimax_api_key" ? "API Key" : "Token Plan"}
          {" · "}
          {t(status.hasApiKey ? "mcode.keySaved" : "mcode.noKey")}
        </p>
      )}
      <form
        className="space-y-2"
        onSubmit={(event) => {
          event.preventDefault();
          if (!busy && apiKey.trim()) void run("saveApiKey", apiKey.trim());
        }}
      >
        <Label htmlFor="mcode-api-key">MiniMax API Key</Label>
        <div className="flex gap-2">
          <Input
            id="mcode-api-key"
            type="password"
            autoComplete="off"
            value={apiKey}
            disabled={busy}
            onChange={(event) => setApiKey(event.target.value)}
          />
          <Button type="submit" disabled={busy || !apiKey.trim()}>
            {t("mcode.saveAndUse")}
          </Button>
        </div>
      </form>
      <div className="flex flex-wrap gap-2">
        <Button
          variant="outline"
          disabled={busy || !status?.hasApiKey}
          onClick={() => void run("useApiKey")}
        >
          {t("mcode.useApiKey")}
        </Button>
        <Button
          variant="outline"
          disabled={busy || !status}
          onClick={() => void run("useTokenPlan")}
        >
          {t("mcode.useTokenPlan")}
        </Button>
        <Button
          variant="outline"
          disabled={busy || !status?.hasApiKey}
          onClick={() => void run("test")}
        >
          {t("mcode.test")}
        </Button>
        <Button
          variant="ghost"
          disabled={busy}
          onClick={() => void run("status")}
        >
          {t("common.refresh")}
        </Button>
      </div>
      <p className="text-sm text-muted-foreground">{t("mcode.loginHint")}</p>
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      <p role="status" className="text-sm text-muted-foreground">
        {busy ? t("mcode.working") : message ? t(message) : ""}
      </p>
    </section>
  );
}
