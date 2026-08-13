import { useEffect, useMemo, useState } from "react";
import { Eye, EyeOff, RefreshCw } from "lucide-react";
import type {
  DshCredentialInfo,
  DshCustomInput,
  DshModel,
  DshNativeInput,
  DshProvider,
} from "@/lib/api/dsh";
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
  readOnly?: boolean;
  onClose: () => void;
  onSaveNative: (
    input: DshNativeInput,
    apiKey?: { ref: string; value: string },
  ) => Promise<void>;
  onSaveCustom: (
    input: DshCustomInput,
    apiKey?: { ref: string; value: string },
  ) => Promise<void>;
  onDiscover: (input: {
    baseURL: string;
    api: string;
    apiKey?: string;
  }) => Promise<DshModel[]>;
}

function credentialLabel(credential?: DshCredentialInfo): string {
  if (!credential) return "未配置 API key";
  if (!credential.configured) return `未配置（${credential.ref}）`;
  if (credential.source === "env" && !credential.writable)
    return `由启动环境提供（${credential.ref}）`;
  return `已配置（${credential.source ?? "managed"}）`;
}

/** Native/custom DSH route editor. API keys are held only in this dialog. */
export function DshProviderDialog({
  open,
  provider,
  protocols,
  readOnly = false,
  onClose,
  onSaveNative,
  onSaveCustom,
  onDiscover,
}: DshProviderDialogProps) {
  const isNative = provider?.kind === "native";
  const isCreate = provider === null;
  const [route, setRoute] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [api, setApi] = useState(protocols[0] ?? DSH_PROTOCOLS[0]);
  const [baseURL, setBaseURL] = useState("");
  const [models, setModels] = useState<DshModel[]>([]);
  const [apiKey, setApiKey] = useState("");
  const [showKey, setShowKey] = useState(false);
  const [busy, setBusy] = useState(false);
  const [discovering, setDiscovering] = useState(false);
  const [failure, setFailure] = useState<string | undefined>();

  useEffect(() => {
    if (!open) {
      setApiKey("");
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
    setFailure(undefined);
  }, [open, provider, protocols]);

  const credential = provider?.credential;
  const keyError = validateDshApiKey(apiKey);
  const routeError =
    !isNative && !isCreate
      ? validateDshRoute(route)
      : isCreate
        ? validateDshRoute(route)
        : undefined;
  const modelError = validateDshModels(models);
  const baseError =
    !isNative && !baseURL.trim() ? "Base URL 不能为空" : undefined;
  const disabled = readOnly || busy || (provider?.kind === "native" && false);
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

  const discover = async () => {
    if (!baseURL.trim() || !api) {
      setFailure("请先填写 Base URL 和协议");
      return;
    }
    setDiscovering(true);
    setFailure(undefined);
    try {
      const discovered = await onDiscover({
        baseURL: baseURL.trim(),
        api,
        apiKey: apiKey.trim() || undefined,
      });
      setModels((current) => {
        const known = new Set(current.map((model) => model.id));
        return [
          ...current,
          ...discovered.filter((model) => !known.has(model.id)),
        ];
      });
    } catch (error) {
      setFailure(
        error instanceof Error ? error.message : "模型读取失败；请手动填写模型",
      );
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
        ? { ref: effectiveRef, value: trimmedKey }
        : undefined;
      if (isNative && provider) {
        await onSaveNative(
          {
            baseURL: baseURL.trim() || undefined,
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
      setFailure(error instanceof Error ? error.message : "保存失败");
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
              ? "DeepSeek Official 设置"
              : isCreate
                ? "添加 DSH Provider"
                : `编辑 ${provider?.displayName}`}
          </DialogTitle>
          <DialogDescription>
            配置直接写入 DSH settings.yaml；cc-switch 不保存 provider 或 API
            key。
          </DialogDescription>
        </DialogHeader>
        <div className="max-h-[65vh] space-y-5 overflow-y-auto px-6 py-5">
          {readOnly && (
            <Alert>
              <AlertDescription>
                当前 DSH 配置为只读。可以查看，但不能写入。
              </AlertDescription>
            </Alert>
          )}
          {failure && (
            <Alert variant="destructive">
              <AlertDescription>{failure}</AlertDescription>
            </Alert>
          )}
          {!isNative && (
            <div className="space-y-1.5">
              <Label htmlFor="dsh-route">Provider ID</Label>
              <Input
                id="dsh-route"
                value={route}
                disabled={!isCreate || disabled}
                onChange={(event) => setRoute(event.target.value)}
                placeholder="my-gateway"
              />
              {routeError && (
                <p className="text-xs text-destructive">{routeError}</p>
              )}
            </div>
          )}
          {!isNative && (
            <div className="space-y-1.5">
              <Label htmlFor="dsh-display-name">显示名称（可选）</Label>
              <Input
                id="dsh-display-name"
                value={displayName}
                disabled={disabled}
                onChange={(event) => setDisplayName(event.target.value)}
                placeholder={route || "Provider"}
              />
            </div>
          )}
          {!isNative && (
            <div className="space-y-1.5">
              <Label>协议</Label>
              <Select value={api} onValueChange={setApi} disabled={disabled}>
                <SelectTrigger>
                  <SelectValue placeholder="选择协议" />
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
              Base URL{isNative ? "（可选）" : ""}
            </Label>
            <Input
              id="dsh-base-url"
              value={baseURL}
              disabled={disabled}
              onChange={(event) => setBaseURL(event.target.value)}
              placeholder="https://api.deepseek.com"
            />
            {baseError && (
              <p className="text-xs text-destructive">{baseError}</p>
            )}
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="dsh-api-key">API key（可选，仅写入时填写）</Label>
            <div className="flex gap-2">
              <Input
                id="dsh-api-key"
                type={showKey ? "text" : "password"}
                value={apiKey}
                disabled={
                  disabled ||
                  (credential?.source === "env" && !credential.writable)
                }
                onChange={(event) => setApiKey(event.target.value)}
                placeholder={credentialLabel(credential)}
                aria-invalid={Boolean(keyError)}
              />
              <Button
                type="button"
                variant="outline"
                size="icon"
                onClick={() => setShowKey((value) => !value)}
                aria-label={showKey ? "隐藏 API key" : "显示 API key"}
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
            {credential?.source === "env" && !credential.writable && (
              <p className="text-xs text-muted-foreground">
                该 key 来自启动环境，不能由 DSH credentials 文件覆盖。
              </p>
            )}
            {keyError && <p className="text-xs text-destructive">{keyError}</p>}
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
            取消
          </Button>
          <Button
            type="button"
            onClick={() => void save()}
            disabled={!canSubmit}
          >
            {busy ? <RefreshCw className="h-4 w-4 animate-spin" /> : null}保存
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
