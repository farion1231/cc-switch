import { useEffect, useState } from "react";
import { Loader2, ShieldCheck, WandSparkles } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type { AppId } from "@/lib/api";
import {
  providerWizardApi,
  type ApplyProviderInstallResult,
  type ProviderInstallPreview,
  type ProviderProbeResult,
  type UpstreamProtocol,
} from "@/lib/api/provider-wizard";

interface ProviderSetupWizardProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  initialApp: AppId;
}

const PROTOCOL_LABELS: Record<UpstreamProtocol, string> = {
  anthropic_messages: "Anthropic Messages",
  open_ai_chat: "OpenAI Chat Completions",
  open_ai_responses: "OpenAI Responses",
};

function preferredProtocol(
  probe: ProviderProbeResult,
  app: "claude" | "codex",
): UpstreamProtocol | undefined {
  const preference: UpstreamProtocol[] =
    app === "claude"
      ? ["anthropic_messages", "open_ai_responses", "open_ai_chat"]
      : ["open_ai_responses", "open_ai_chat", "anthropic_messages"];
  return preference.find((protocol) =>
    probe.capabilities.some(
      (capability) => capability.protocol === protocol && capability.supported,
    ),
  );
}

export function ProviderSetupWizard({
  open,
  onOpenChange,
  initialApp,
}: ProviderSetupWizardProps) {
  const queryClient = useQueryClient();
  const [name, setName] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [model, setModel] = useState("");
  const [probe, setProbe] = useState<ProviderProbeResult | null>(null);
  const [preview, setPreview] = useState<ProviderInstallPreview | null>(null);
  const [claudeSelected, setClaudeSelected] = useState(initialApp === "claude");
  const [codexSelected, setCodexSelected] = useState(initialApp === "codex");
  const [claudeProtocol, setClaudeProtocol] = useState<
    UpstreamProtocol | undefined
  >();
  const [codexProtocol, setCodexProtocol] = useState<
    UpstreamProtocol | undefined
  >();
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) {
      setName("");
      setBaseUrl("");
      setApiKey("");
      setModel("");
      setProbe(null);
      setPreview(null);
      setClaudeSelected(initialApp === "claude");
      setCodexSelected(initialApp === "codex");
      setClaudeProtocol(undefined);
      setCodexProtocol(undefined);
    }
  }, [initialApp, open]);

  const runProbe = async () => {
    if (!name.trim() || !baseUrl.trim() || !apiKey.trim()) {
      toast.error("Nhập tên, Base URL và API key trước khi kiểm tra.");
      return;
    }
    setBusy(true);
    try {
      const result = await providerWizardApi.probe({
        baseUrl,
        apiKey,
        model: model || undefined,
        allowInferenceProbe: true,
      });
      setProbe(result);
      setModel(result.recommendedModel ?? model);
      setClaudeProtocol(preferredProtocol(result, "claude"));
      setCodexProtocol(preferredProtocol(result, "codex"));
      setPreview(null);
      toast.success("Đã kiểm tra protocol và model.");
    } catch (error) {
      toast.error(String(error));
    } finally {
      setBusy(false);
    }
  };

  const buildPreview = async () => {
    if (!probe || !model.trim() || (!claudeSelected && !codexSelected)) {
      toast.error("Chọn ứng dụng và model trước khi xem preview.");
      return;
    }
    setBusy(true);
    try {
      const result = await providerWizardApi.preview({
        name,
        baseUrl,
        apiKey,
        model,
        claudeProtocol: claudeSelected ? claudeProtocol : undefined,
        codexProtocol: codexSelected ? codexProtocol : undefined,
      });
      setPreview(result);
    } catch (error) {
      toast.error(String(error));
    } finally {
      setBusy(false);
    }
  };

  const apply = async () => {
    if (!preview) return;
    setBusy(true);
    try {
      const result: ApplyProviderInstallResult = await providerWizardApi.apply({
        name,
        baseUrl,
        apiKey,
        model,
        claudeProtocol: claudeSelected ? claudeProtocol : undefined,
        codexProtocol: codexSelected ? codexProtocol : undefined,
      });
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["providers", "claude"] }),
        queryClient.invalidateQueries({ queryKey: ["providers", "codex"] }),
      ]);
      toast.success(
        `Đã cài ${result.appliedApps.length} ứng dụng. Hãy mở lại IDE nếu được yêu cầu.`,
      );
      onOpenChange(false);
    } catch (error) {
      toast.error(String(error), { duration: 8000 });
    } finally {
      setBusy(false);
    }
  };

  const canPreview =
    !!probe &&
    !!model.trim() &&
    (claudeSelected || codexSelected) &&
    (!claudeSelected || !!claudeProtocol) &&
    (!codexSelected || !!codexProtocol);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <WandSparkles className="h-5 w-5" /> Thiết lập provider nhanh
          </DialogTitle>
          <DialogDescription>
            Nhập thông tin kết nối. CC Switch sẽ kiểm tra sau khi bạn xác nhận,
            nhưng chưa thay đổi cấu hình trước bước Apply.
          </DialogDescription>
        </DialogHeader>

        <div className="grid gap-4 overflow-y-auto py-2">
          <div className="grid gap-2">
            <Label htmlFor="wizard-provider-name">Tên provider</Label>
            <Input
              id="wizard-provider-name"
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder="Ví dụ: My Gateway"
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="wizard-base-url">Base URL hoặc full endpoint</Label>
            <Input
              id="wizard-base-url"
              value={baseUrl}
              onChange={(event) => setBaseUrl(event.target.value)}
              placeholder="https://provider.example/v1"
              type="url"
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="wizard-api-key">API key</Label>
            <Input
              id="wizard-api-key"
              value={apiKey}
              onChange={(event) => setApiKey(event.target.value)}
              placeholder="Key chỉ được giữ trong phiên thiết lập"
              type="password"
              autoComplete="off"
            />
          </div>

          <div className="rounded-lg border border-amber-500/30 bg-amber-500/5 p-3 text-sm">
            Probe sẽ gửi tối đa vài request nhỏ để kiểm tra `/models` và
            protocol. Có thể phát sinh một lượng token rất nhỏ.
          </div>

          <Button onClick={runProbe} disabled={busy} variant="secondary">
            {busy && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
            Cho phép kiểm tra kết nối
          </Button>

          {probe && (
            <div className="grid gap-3 rounded-lg border p-3">
              <div className="flex items-center gap-2 text-sm font-medium">
                <ShieldCheck className="h-4 w-4 text-emerald-500" />
                {probe.models.length} model được phát hiện tại{" "}
                {probe.normalizedBaseUrl}
              </div>
              <div className="grid gap-2 sm:grid-cols-2">
                {(["claude", "codex"] as const).map((app) => {
                  const selected =
                    app === "claude" ? claudeSelected : codexSelected;
                  const protocol =
                    app === "claude" ? claudeProtocol : codexProtocol;
                  const setSelected =
                    app === "claude" ? setClaudeSelected : setCodexSelected;
                  const setProtocol =
                    app === "claude" ? setClaudeProtocol : setCodexProtocol;
                  return (
                    <div key={app} className="grid gap-2 rounded-md border p-3">
                      <label className="flex items-center gap-2 text-sm font-medium">
                        <input
                          type="checkbox"
                          checked={selected}
                          onChange={(event) =>
                            setSelected(event.target.checked)
                          }
                        />
                        {app === "claude" ? "Claude Code" : "Codex"}
                      </label>
                      <select
                        className="h-9 rounded-md border bg-background px-2 text-sm"
                        value={protocol ?? ""}
                        disabled={!selected}
                        onChange={(event) =>
                          setProtocol(
                            (event.target.value || undefined) as
                              | UpstreamProtocol
                              | undefined,
                          )
                        }
                      >
                        <option value="">Chọn protocol</option>
                        {probe.capabilities
                          .filter((capability) => capability.supported)
                          .map((capability) => (
                            <option
                              key={capability.protocol}
                              value={capability.protocol}
                            >
                              {PROTOCOL_LABELS[capability.protocol]}
                            </option>
                          ))}
                      </select>
                    </div>
                  );
                })}
              </div>
              <div className="grid gap-2">
                <Label htmlFor="wizard-model">Model sử dụng</Label>
                <Input
                  id="wizard-model"
                  value={model}
                  onChange={(event) => setModel(event.target.value)}
                  list="wizard-model-list"
                />
                <datalist id="wizard-model-list">
                  {probe.models.map((item) => (
                    <option key={item.id} value={item.id} />
                  ))}
                </datalist>
              </div>
              <Button onClick={buildPreview} disabled={busy || !canPreview}>
                Xem preview cấu hình
              </Button>
            </div>
          )}

          {preview && (
            <div className="grid gap-2 rounded-lg border border-primary/30 bg-primary/5 p-3 text-sm">
              <strong>Preview</strong>
              {[preview.claude, preview.codex].filter(Boolean).map((item) => (
                <div key={item!.app}>
                  <b>{item!.app === "claude" ? "Claude Code" : "Codex"}</b>:{" "}
                  {item!.mode}, {PROTOCOL_LABELS[item!.protocol]}, model `
                  {item!.model}`
                  <div className="text-muted-foreground">
                    {item!.filesToChange.join(", ")}
                  </div>
                </div>
              ))}
              {preview.proxyWillStart && (
                <div className="text-amber-600">
                  Một ứng dụng cần local routing. Apply sẽ bật takeover sau khi
                  bạn xác nhận.
                </div>
              )}
            </div>
          )}
        </div>

        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={busy}
          >
            Hủy
          </Button>
          <Button onClick={apply} disabled={busy || !preview}>
            {busy && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
            Apply cấu hình
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
