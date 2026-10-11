import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { FolderOpen, Loader2, Play } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { settingsApi } from "@/lib/api";
import {
  apiKeyVarsApi,
  isValidApiKeyVarName,
  type ApiKeyVar,
  type ApiKeyVarInput,
  type ApiKeyVarTestOutcome,
  type ResolvedScript,
} from "@/lib/api/apiKeyVars";
import { extractErrorMessage } from "@/utils/errorUtils";

interface ApiKeyVarDialogProps {
  open: boolean;
  /** 编辑时传入；新增时为 undefined */
  variable?: ApiKeyVar;
  existingNames: string[];
  onClose: () => void;
  onSaved: (saved: ApiKeyVar, wasEnabled: boolean) => void;
  onOpenGuide: () => void;
}

const parseOptionalInt = (value: string): number | undefined => {
  const trimmed = value.trim();
  if (!trimmed) return undefined;
  const n = Number(trimmed);
  return Number.isFinite(n) && n >= 0 ? Math.floor(n) : undefined;
};

export function ApiKeyVarDialog({
  open,
  variable,
  existingNames,
  onClose,
  onSaved,
  onOpenGuide,
}: ApiKeyVarDialogProps) {
  const { t } = useTranslation();
  const [name, setName] = useState("");
  const [scriptDir, setScriptDir] = useState("");
  const [enabled, setEnabled] = useState(true);
  const [timeout, setTimeoutValue] = useState("");
  const [leadMinutes, setLeadMinutes] = useState("");
  const [resolved, setResolved] = useState<ResolvedScript | null>(null);
  const [resolveError, setResolveError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<ApiKeyVarTestOutcome | null>(
    null,
  );
  const [testError, setTestError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [confirmOpen, setConfirmOpen] = useState(false);

  useEffect(() => {
    if (!open) return;
    setName(variable?.name ?? "");
    setScriptDir(variable?.scriptDir ?? "");
    setEnabled(variable?.enabled ?? true);
    setTimeoutValue(
      variable?.timeoutSecs !== undefined ? String(variable.timeoutSecs) : "",
    );
    setLeadMinutes(
      variable?.leadTimeSecs !== undefined
        ? String(Math.round(variable.leadTimeSecs / 60))
        : "",
    );
    setTestResult(null);
    setTestError(null);
    setSaveError(null);
  }, [open, variable]);

  // 目录变化时提示将运行哪个文件
  useEffect(() => {
    if (!open) return;
    const dir = scriptDir.trim();
    if (!dir) {
      setResolved(null);
      setResolveError(null);
      return;
    }
    let cancelled = false;
    apiKeyVarsApi
      .resolveScript(dir)
      .then((r) => {
        if (cancelled) return;
        setResolved(r);
        setResolveError(null);
      })
      .catch((e) => {
        if (cancelled) return;
        setResolved(null);
        setResolveError(extractErrorMessage(e));
      });
    return () => {
      cancelled = true;
    };
  }, [open, scriptDir]);

  const trimmedName = name.trim();
  const nameError = useMemo(() => {
    if (!trimmedName) return null;
    if (!isValidApiKeyVarName(trimmedName)) {
      return t("apiKeyVars.nameInvalid");
    }
    if (trimmedName !== variable?.name && existingNames.includes(trimmedName)) {
      return t("apiKeyVars.nameDuplicate");
    }
    return null;
  }, [trimmedName, variable?.name, existingNames, t]);

  const renaming = Boolean(variable && trimmedName !== variable.name);
  const refCount = variable?.references.length ?? 0;

  const buildInput = (): ApiKeyVarInput => {
    const lead = parseOptionalInt(leadMinutes);
    return {
      name: trimmedName,
      scriptDir: scriptDir.trim(),
      enabled,
      timeoutSecs: parseOptionalInt(timeout),
      leadTimeSecs: lead !== undefined ? lead * 60 : undefined,
    };
  };

  const canSave =
    Boolean(trimmedName) &&
    !nameError &&
    (!enabled || (Boolean(scriptDir.trim()) && !resolveError));

  const handleBrowse = async () => {
    try {
      const picked = await settingsApi.selectConfigDirectory(
        scriptDir.trim() || undefined,
      );
      if (picked) setScriptDir(picked);
    } catch (e) {
      setResolveError(extractErrorMessage(e));
    }
  };

  const handleTest = async () => {
    setTesting(true);
    setTestResult(null);
    setTestError(null);
    try {
      setTestResult(await apiKeyVarsApi.test(buildInput()));
    } catch (e) {
      setTestError(extractErrorMessage(e));
    } finally {
      setTesting(false);
    }
  };

  const doSave = async () => {
    setSaving(true);
    setSaveError(null);
    try {
      const saved = await apiKeyVarsApi.upsert(buildInput(), variable?.name);
      onSaved(saved, Boolean(variable?.enabled));
    } catch (e) {
      setSaveError(extractErrorMessage(e));
    } finally {
      setSaving(false);
    }
  };

  const handleSave = () => {
    // 首次启用：确认会在本机运行脚本
    if (enabled && !variable?.enabled) {
      setConfirmOpen(true);
      return;
    }
    void doSave();
  };

  const formatTime = (ms?: number) =>
    ms ? new Date(ms).toLocaleString() : t("apiKeyVars.noExpiry");

  return (
    <>
      <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
        <DialogContent className="max-w-lg">
          <DialogHeader>
            <DialogTitle>
              {variable ? t("apiKeyVars.editTitle") : t("apiKeyVars.addTitle")}
            </DialogTitle>
            <DialogDescription className="sr-only">
              {t("apiKeyVars.nameHint", {
                ref: `{{${trimmedName || "MY_KEY"}}}`,
              })}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4 px-6 py-2">
            <div className="space-y-1.5">
              <label
                htmlFor="api-key-var-name"
                className="block text-caption font-medium text-fg-1"
              >
                {t("apiKeyVars.name")}
              </label>
              <Input
                id="api-key-var-name"
                value={name}
                onChange={(e) => setName(e.target.value.toUpperCase())}
                placeholder="MY_KEY"
                className="font-mono"
                autoComplete="off"
              />
              {nameError ? (
                <p className="text-caption text-danger-text">{nameError}</p>
              ) : (
                <p className="text-caption text-fg-2">
                  {t("apiKeyVars.nameHint", {
                    ref: `{{${trimmedName || "MY_KEY"}}}`,
                  })}
                </p>
              )}
              {renaming && refCount > 0 && (
                <p className="text-caption text-warning-text">
                  {t("apiKeyVars.renameRewrites", { count: refCount })}
                </p>
              )}
            </div>

            <div className="space-y-1.5">
              <label className="block text-caption font-medium text-fg-1">
                {t("apiKeyVars.scriptDir")}
              </label>
              <div className="flex gap-2">
                <Input
                  value={scriptDir}
                  onChange={(e) => setScriptDir(e.target.value)}
                  placeholder={t("apiKeyVars.scriptDirPlaceholder")}
                  className="font-mono"
                />
                <Button
                  type="button"
                  variant="neutral"
                  size="regular"
                  onClick={() => void handleBrowse()}
                >
                  <FolderOpen className="h-4 w-4" />
                  {t("apiKeyVars.chooseDir")}
                </Button>
              </div>
              <Button
                type="button"
                variant="link"
                size="compact"
                className="h-auto px-0 text-caption"
                onClick={onOpenGuide}
              >
                {t("apiKeyVars.template.openGuide")}
              </Button>
              {resolveError ? (
                <p className="text-caption text-danger-text">{resolveError}</p>
              ) : resolved ? (
                <p className="text-caption text-fg-2">
                  {t("apiKeyVars.willRun", { file: resolved.file })}
                  {resolved.ignoredIndexJs &&
                    ` · ${t("apiKeyVars.indexJsIgnored")}`}
                </p>
              ) : null}
            </div>

            <div className="flex items-center justify-between">
              <span className="text-caption font-medium text-fg-1">
                {t("apiKeyVars.enable")}
              </span>
              <Switch checked={enabled} onCheckedChange={setEnabled} />
            </div>

            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <label className="block text-caption font-medium text-fg-1">
                  {t("apiKeyVars.timeout")}
                </label>
                <Input
                  inputMode="numeric"
                  value={timeout}
                  onChange={(e) => setTimeoutValue(e.target.value)}
                  placeholder="30"
                />
              </div>
              <div className="space-y-1.5">
                <label className="block text-caption font-medium text-fg-1">
                  {t("apiKeyVars.leadTime")}
                </label>
                <Input
                  inputMode="numeric"
                  value={leadMinutes}
                  onChange={(e) => setLeadMinutes(e.target.value)}
                  placeholder="5"
                />
              </div>
            </div>

            <div className="space-y-1.5">
              <Button
                type="button"
                variant="neutral"
                size="compact"
                disabled={!scriptDir.trim() || Boolean(resolveError) || testing}
                onClick={() => void handleTest()}
              >
                {testing ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <Play className="h-3.5 w-3.5" />
                )}
                {t("apiKeyVars.test")}
              </Button>
              {testResult && (
                <p className="text-caption text-fg-2">
                  {t("apiKeyVars.testSuccess", {
                    value: testResult.apiKeyMasked,
                    time: formatTime(testResult.expiresAt),
                    file: testResult.file,
                  })}
                  {testResult.message ? ` · ${testResult.message}` : ""}
                </p>
              )}
              {testError && (
                <p className="text-caption text-danger-text break-all">
                  {testError}
                </p>
              )}
            </div>
            {saveError && (
              <p className="text-caption text-danger-text break-all">
                {saveError}
              </p>
            )}
          </div>
          <DialogFooter>
            <Button variant="neutral" size="regular" onClick={onClose}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="solid"
              size="regular"
              disabled={!canSave || saving}
              onClick={handleSave}
            >
              {saving && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
              {t("common.save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <ConfirmDialog
        isOpen={confirmOpen}
        variant="info"
        zIndex="alert"
        title={t("apiKeyVars.securityTitle")}
        message={t("apiKeyVars.securityWarning", { dir: scriptDir.trim() })}
        confirmText={t("apiKeyVars.securityConfirm")}
        onConfirm={() => {
          setConfirmOpen(false);
          void doSave();
        }}
        onCancel={() => setConfirmOpen(false)}
      />
    </>
  );
}
