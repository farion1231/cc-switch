import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Eye, EyeOff, RefreshCw } from "lucide-react";
import type {
  DshCredentialInfo,
  DshCustomInput,
  DshModel,
  DshNativeInput,
  DshProvider,
} from "@/lib/api/dsh";
import { dshErrorMessage, isDshConflictError } from "@/lib/api/dsh";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { DshModelEditor } from "./DshModelEditor";
import {
  deriveDshCredentialRef,
  DSH_PROTOCOLS,
  validateDshApiKey,
  validateDshModels,
  validateDshRoute,
} from "./dshModelUtils";

interface DshProviderDialogProps {
  open: boolean;
  provider: DshProvider | null;
  protocols: readonly string[];
  credentialsRevision?: string;
  readOnly?: boolean;
  onClose: () => void;
  onSaveNative: (
    input: DshNativeInput,
    apiKey?: { ref: string; value: string; expectedRevision?: string },
  ) => Promise<void>;
  onSaveCustom: (
    input: DshCustomInput,
    apiKey?: { ref: string; value: string; expectedRevision?: string },
  ) => Promise<void>;
  onUnsetCredential?: (ref: string, expectedRevision?: string) => Promise<void>;
  onDiscover: (input: {
    baseURL: string;
    api: string;
    apiKey?: string;
    credentialRef?: string;
  }) => Promise<DshModel[]>;
}

function credentialLabel(
  t: ReturnType<typeof useTranslation>["t"],
  credential?: DshCredentialInfo,
): string {
  if (!credential) return t("dsh.credentials.notConfigured");
  if (!credential.configured)
    return t("dsh.credentials.notConfiguredRef", { ref: credential.ref });
  if (credential.source === "process" && !credential.writable)
    return t("dsh.credentials.environmentRef", { ref: credential.ref });
  return t("dsh.credentials.configuredSource", {
    source: credential.source ?? t("dsh.credentials.managed"),
  });
}

/** Native/custom DSH route editor. API keys are held only in this dialog. */
export function DshProviderDialog({
  open,
  provider,
  protocols,
  credentialsRevision,
  readOnly = false,
  onClose,
  onSaveNative,
  onSaveCustom,
  onUnsetCredential,
  onDiscover,
}: DshProviderDialogProps) {
  const { t } = useTranslation();
  const isNative = provider?.kind === "native";
  const isCreate = provider === null;
  const [route, setRoute] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [api, setApi] = useState(protocols[0] ?? DSH_PROTOCOLS[0]);
  const [baseURL, setBaseURL] = useState("");
  const [models, setModels] = useState<DshModel[]>([]);
  const [apiKey, setApiKey] = useState("");
  const [showKey, setShowKey] = useState(false);
  const [confirmRemoveKey, setConfirmRemoveKey] = useState(false);
  const [busy, setBusy] = useState(false);
  const [discovering, setDiscovering] = useState(false);
  const [failure, setFailure] = useState<string | undefined>();

  useEffect(() => {
    if (!open) {
      setApiKey("");
      setConfirmRemoveKey(false);
      setFailure(undefined);
      return;
    }
    setRoute(provider?.route ?? "");
    setDisplayName(
      provider?.displayName === provider?.route
        ? ""
        : (provider?.displayName ?? ""),
    );
    setApi(provider?.api ?? protocols[0] ?? DSH_PROTOCOLS[0]);
    setBaseURL(
      provider?.baseURL ??
        (provider?.kind === "native" ? "https://api.deepseek.com" : ""),
    );
    setModels(provider?.models.map((model) => ({ ...model })) ?? []);
    setApiKey("");
    setShowKey(false);
    setConfirmRemoveKey(false);
    setFailure(undefined);
  }, [open, provider, protocols]);

  const credential = provider?.credential;
  // File-backed keys can be removed explicitly; process-environment keys are
  // read-only and are rejected by the backend.
  const canRemoveKey =
    Boolean(credential?.configured) &&
    credential?.source === "file" &&
    Boolean(credential.writable);
  const keyError = validateDshApiKey(apiKey);
  // Existing routes can use identifiers written by DSH itself.  The create
  // form uses the UI's lower-kebab convention, while an edit must not reject
  // an already valid nonstandard identifier merely because the route field is
  // immutable there.
  const routeError =
    !isNative && isCreate ? validateDshRoute(route) : undefined;
  const modelError = validateDshModels(models);
  const baseError =
    !isNative && !baseURL.trim() ? "dsh.validation.baseUrlRequired" : undefined;
  const disabled = readOnly || busy;
  const canSubmit =
    !disabled && !keyError && !routeError && !modelError && !baseError;
  const effectiveRef =
    credential?.ref ??
    (route ? deriveDshCredentialRef(route) : "DEEPSEEK_API_KEY");
  const protocolChoices = useMemo(() => {
    const values = [...protocols];
    for (const protocol of DSH_PROTOCOLS)
      if (!values.includes(protocol)) values.push(protocol);
    return values;
  }, [protocols]);
  const displayError = (error: unknown, fallbackKey: string) =>
    isDshConflictError(error)
      ? t("dsh.errors.conflict")
      : dshErrorMessage(error, t(fallbackKey));

  const discover = async () => {
    if (!baseURL.trim() || !api) {
      setFailure(t("dsh.validation.discoveryFieldsRequired"));
      return;
    }
    setDiscovering(true);
    setFailure(undefined);
    try {
      const discovered = await onDiscover({
        baseURL: baseURL.trim(),
        api,
        apiKey: apiKey.trim() || undefined,
        credentialRef: provider?.apiKeyEnv,
      });
      setModels((current) => {
        const known = new Set(current.map((model) => model.id));
        return [
          ...current,
          ...discovered.filter((model) => !known.has(model.id)),
        ];
      });
    } catch (error) {
      setFailure(displayError(error, "dsh.errors.modelDiscovery"));
    } finally {
      setDiscovering(false);
    }
  };

  const save = async () => {
    if (!canSubmit) return;
    setBusy(true);
    setFailure(undefined);
    try {
      const trimmedKey = apiKey.trim();
      const keyPayload = trimmedKey
        ? {
            ref: effectiveRef,
            value: trimmedKey,
            expectedRevision: credentialsRevision,
          }
        : undefined;
      if (isNative && provider) {
        await onSaveNative(
          {
            // An empty Base URL clears a stored native override; the backend
            // distinguishes "not sent" from "explicitly cleared".
            baseURL: baseURL.trim(),
            models,
            apiKeyEnv: provider.apiKeyEnv,
            expectedRevision: provider.revision,
          },
          keyPayload,
        );
      } else {
        await onSaveCustom(
          {
            route: route.trim(),
            displayName: displayName.trim() || undefined,
            api,
            baseURL: baseURL.trim(),
            models,
            apiKeyEnv: trimmedKey ? effectiveRef : provider?.apiKeyEnv,
            expectedRevision: provider?.revision,
          },
          keyPayload,
        );
      }
      setApiKey("");
      onClose();
    } catch (error) {
      setFailure(displayError(error, "dsh.errors.save"));
    } finally {
      setBusy(false);
    }
  };

  const removeKey = async () => {
    // Two-step confirmation: the first click arms the destructive action, the
    // second executes it. Typing a replacement key disarms it again.
    if (!confirmRemoveKey) {
      setConfirmRemoveKey(true);
      return;
    }
    setBusy(true);
    setFailure(undefined);
    try {
      await onUnsetCredential?.(effectiveRef, credentialsRevision);
      setApiKey("");
      setConfirmRemoveKey(false);
    } catch (error) {
      setFailure(displayError(error, "dsh.credentials.removeFailed"));
    } finally {
      setBusy(false);
    }
  };

  if (!open) return null;

  return (
    <Dialog open={open} onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>
            {isNative
              ? t("dsh.dialog.nativeTitle")
              : isCreate
                ? t("dsh.dialog.addTitle")
                : t("dsh.dialog.editTitle", {
                    name: provider?.displayName,
                  })}
          </DialogTitle>
          <DialogDescription>{t("dsh.dialog.description")}</DialogDescription>
        </DialogHeader>
        <div className="max-h-[65vh] space-y-5 overflow-y-auto px-6 py-5">
          {readOnly && (
            <Alert>
              <AlertDescription>{t("dsh.dialog.readOnly")}</AlertDescription>
            </Alert>
          )}
          {failure && (
            <Alert variant="destructive">
              <AlertDescription>{failure}</AlertDescription>
            </Alert>
          )}
          {!isNative && (
            <div className="space-y-1.5">
              <Label htmlFor="dsh-route">{t("dsh.fields.providerId")}</Label>
              <Input
                id="dsh-route"
                value={route}
                disabled={!isCreate || disabled}
                onChange={(event) => setRoute(event.target.value)}
                placeholder="my-gateway"
              />
              {routeError && (
                <p className="text-xs text-destructive">{t(routeError)}</p>
              )}
            </div>
          )}
          {!isNative && (
            <div className="space-y-1.5">
              <Label htmlFor="dsh-display-name">
                {t("dsh.fields.displayName")}
              </Label>
              <Input
                id="dsh-display-name"
                value={displayName}
                disabled={disabled}
                onChange={(event) => setDisplayName(event.target.value)}
                placeholder={route || t("dsh.fields.providerPlaceholder")}
              />
            </div>
          )}
          {!isNative && (
            <div className="space-y-1.5">
              <Label>{t("dsh.fields.protocol")}</Label>
              <Select value={api} onValueChange={setApi} disabled={disabled}>
                <SelectTrigger>
                  <SelectValue placeholder={t("dsh.fields.selectProtocol")} />
                </SelectTrigger>
                <SelectContent>
                  {protocolChoices.map((value) => (
                    <SelectItem key={value} value={value}>
                      {value}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}
          <div className="space-y-1.5">
            <Label htmlFor="dsh-base-url">
              {isNative
                ? t("dsh.fields.baseUrlOptional")
                : t("dsh.fields.baseUrl")}
            </Label>
            <Input
              id="dsh-base-url"
              value={baseURL}
              disabled={disabled}
              onChange={(event) => setBaseURL(event.target.value)}
              placeholder="https://api.deepseek.com"
            />
            {baseError && (
              <p className="text-xs text-destructive">{t(baseError)}</p>
            )}
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="dsh-api-key">{t("dsh.fields.apiKey")}</Label>
            <div className="flex gap-2">
              <Input
                id="dsh-api-key"
                type={showKey ? "text" : "password"}
                value={apiKey}
                disabled={
                  disabled ||
                  (credential?.source === "process" && !credential.writable)
                }
                onChange={(event) => {
                  setApiKey(event.target.value);
                  setConfirmRemoveKey(false);
                }}
                placeholder={credentialLabel(t, credential)}
                aria-invalid={Boolean(keyError)}
              />
              <Button
                type="button"
                variant="outline"
                size="icon"
                onClick={() => setShowKey((value) => !value)}
                aria-label={
                  showKey
                    ? t("dsh.credentials.hide")
                    : t("dsh.credentials.show")
                }
                disabled={disabled}
              >
                <>
                  {showKey ? (
                    <EyeOff className="h-4 w-4" />
                  ) : (
                    <Eye className="h-4 w-4" />
                  )}
                </>
              </Button>
            </div>
            {credential?.source === "process" && !credential.writable && (
              <p className="text-xs text-muted-foreground">
                {t("dsh.credentials.environmentReadOnly")}
              </p>
            )}
            {keyError && (
              <p className="text-xs text-destructive">{t(keyError)}</p>
            )}
            {canRemoveKey && (
              <div className="flex justify-end">
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className={confirmRemoveKey ? "text-destructive" : undefined}
                  onClick={() => void removeKey()}
                  disabled={disabled || busy}
                >
                  {confirmRemoveKey
                    ? t("dsh.credentials.removeConfirm")
                    : t("dsh.credentials.remove")}
                </Button>
              </div>
            )}
          </div>
          <DshModelEditor
            models={models}
            onChange={setModels}
            disabled={disabled}
            discoverable={!isNative && api !== "anthropic-messages"}
            onDiscover={discover}
            discovering={discovering}
          />
        </div>
        <DialogFooter>
          <Button type="button" variant="outline" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button
            type="button"
            onClick={() => void save()}
            disabled={!canSubmit}
          >
            {busy ? <RefreshCw className="h-4 w-4 animate-spin" /> : null}
            {t("common.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
