import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { piApi } from "@/lib/api";
import type {
  PiProviderDraft,
  PiProviderPatchPreview,
  PiProvidersMap,
  PiModelDraft,
  PiHeaderDraft,
} from "@/types/pi";
import {
  emptyPiProviderDraft,
  PiProviderForm,
} from "@/components/pi/PiProviderForm";
import { PiProviderList } from "@/components/pi/PiProviderList";
import { PiProviderDiffPreview } from "@/components/pi/PiProviderDiffPreview";
import { Button } from "@/components/ui/button";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";

export function PiAgentPanel({ addTrigger }: { addTrigger?: number }) {
  const [providers, setProviders] = useState<PiProvidersMap>({});
  const [draft, setDraft] = useState<PiProviderDraft>(emptyPiProviderDraft);
  const [preview, setPreview] = useState<PiProviderPatchPreview | null>(null);
  const [isApplying, setIsApplying] = useState(false);
  const [isEditOpen, setIsEditOpen] = useState(false);
  const [isReviewOpen, setIsReviewOpen] = useState(false);

  const refresh = async () => {
    try {
      setProviders(await piApi.listProviders());
    } catch (error) {
      toast.error("Failed to read Pi models.json", {
        description: String(error),
      });
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  const startNew = () => {
    setDraft({ ...emptyPiProviderDraft });
    setPreview(null);
    setIsEditOpen(true);
  };

  // React to external add trigger (orange "+" button in toolbar)
  const prevTrigger = useRef(addTrigger);
  useEffect(() => {
    if (addTrigger !== undefined && addTrigger !== prevTrigger.current) {
      prevTrigger.current = addTrigger;
      startNew();
    }
  }, [addTrigger]);

  const editProvider = (providerId: string) => {
    const provider = providers[providerId] as
      | Record<string, unknown>
      | undefined;
    if (!provider) {
      startNew();
      return;
    }

    // Parse models array from existing config
    const rawModels = Array.isArray(provider.models) ? provider.models : [];
    const models: PiModelDraft[] =
      rawModels.length > 0
        ? rawModels.map((m: Record<string, unknown>) => ({
            id: String(m.id ?? ""),
            name: typeof m.name === "string" ? m.name : null,
            nameTouched: typeof m.name === "string",
            reasoning: Boolean(m.reasoning),
            input: Array.isArray(m.input) ? (m.input as string[]) : undefined,
            contextWindow:
              typeof m.contextWindow === "number" ? m.contextWindow : undefined,
            maxTokens:
              typeof m.maxTokens === "number" ? m.maxTokens : undefined,
          }))
        : [{ id: "", name: "", nameTouched: false }];

    // Parse headers
    const rawHeaders =
      typeof provider.headers === "object" && provider.headers !== null
        ? (provider.headers as Record<string, unknown>)
        : {};
    const headers: PiHeaderDraft[] = Object.entries(rawHeaders).map(
      ([key, val]) => ({
        key,
        value: String(val ?? ""),
      }),
    );

    // Parse apiKey
    const rawApiKey =
      typeof provider.apiKey === "string" ? provider.apiKey : "";
    let apiKey = emptyPiProviderDraft.apiKey;
    if (rawApiKey.startsWith("$")) {
      apiKey = { mode: "env", value: rawApiKey.slice(1) };
    } else if (rawApiKey.startsWith("!")) {
      apiKey = { mode: "command", value: rawApiKey };
    } else if (rawApiKey) {
      apiKey = { mode: "literal", value: rawApiKey };
    } else {
      apiKey = { mode: "none", value: "" };
    }

    // Parse compat
    const rawCompat =
      typeof provider.compat === "object" && provider.compat !== null
        ? (provider.compat as Record<string, unknown>)
        : null;

    setDraft({
      mode: "custom",
      providerId,
      template: "custom",
      baseUrl: typeof provider.baseUrl === "string" ? provider.baseUrl : "",
      api:
        typeof provider.api === "string" ? provider.api : "openai-completions",
      apiKey,
      headers,
      models,
      compat: rawCompat
        ? {
            supportsDeveloperRole: rawCompat.supportsDeveloperRole as
              | boolean
              | undefined,
            supportsReasoningEffort: rawCompat.supportsReasoningEffort as
              | boolean
              | undefined,
            supportsUsageInStreaming: rawCompat.supportsUsageInStreaming as
              | boolean
              | undefined,
            supportsEagerToolInputStreaming:
              rawCompat.supportsEagerToolInputStreaming as boolean | undefined,
            forceAdaptiveThinking: rawCompat.forceAdaptiveThinking as
              | boolean
              | undefined,
          }
        : null,
      advancedJson: null,
    });
    setPreview(null);
    setIsEditOpen(true);
  };

  const buildPreview = async () => {
    try {
      const next = await piApi.previewProviderPatch(draft);
      setPreview(next);
      setIsEditOpen(false);
      setIsReviewOpen(true);
    } catch (error) {
      toast.error("Failed to preview Pi provider patch", {
        description: String(error),
      });
    }
  };

  const applyPreview = async () => {
    if (!preview) return;
    setIsApplying(true);
    try {
      const result = await piApi.applyProviderPatch(
        draft,
        preview.currentFileHash,
      );
      toast.success("Pi provider saved", {
        description: `Backup: ${result.backupPath}`,
      });
      setPreview(null);
      setIsReviewOpen(false);
      await refresh();
    } catch (error) {
      toast.error("Failed to apply Pi provider patch", {
        description: String(error),
      });
    } finally {
      setIsApplying(false);
    }
  };

  const deletePreview = async () => {
    if (!preview || !draft.providerId.trim()) return;
    setIsApplying(true);
    try {
      const result = await piApi.deleteProvider(
        draft.providerId,
        preview.currentFileHash,
      );
      toast.success("Pi provider deleted", {
        description: `Backup: ${result.backupPath}`,
      });
      setDraft({ ...emptyPiProviderDraft });
      setPreview(null);
      setIsReviewOpen(false);
      await refresh();
    } catch (error) {
      toast.error("Failed to delete Pi provider", {
        description: String(error),
      });
    } finally {
      setIsApplying(false);
    }
  };

  // ─── Duplicate: open edit form with copied data but clear providerId ───────
  const duplicateProvider = (providerId: string) => {
    editProvider(providerId);
    // After editProvider sets the draft, override the providerId to force "new"
    setDraft((prev) => ({
      ...prev,
      providerId: `${prev.providerId}-copy`,
    }));
  };

  // ─── Delete directly from list (creates preview then immediately deletes) ──
  const deleteProviderDirect = async (providerId: string) => {
    try {
      // First get a preview to obtain the current file hash
      const tempDraft: PiProviderDraft = {
        ...emptyPiProviderDraft,
        providerId,
      };
      const previewData = await piApi.previewProviderPatch(tempDraft);
      const result = await piApi.deleteProvider(
        providerId,
        previewData.currentFileHash,
      );
      toast.success("Pi provider deleted", {
        description: `Backup: ${result.backupPath}`,
      });
      await refresh();
    } catch (error) {
      toast.error("Failed to delete Pi provider", {
        description: String(error),
      });
    }
  };

  // ─── Test connectivity: verify the provider's baseUrl is network-reachable ──
  const testConnectivity = async (providerId: string) => {
    const provider = providers[providerId] as
      | Record<string, unknown>
      | undefined;
    const baseUrl =
      typeof provider?.baseUrl === "string" ? provider.baseUrl : "";
    if (!baseUrl) {
      toast.error("No base URL configured for this provider");
      return;
    }

    try {
      // Try the base URL directly (not /models) — some providers like Volcengine
      // don't expose a /models endpoint. Any HTTP response proves reachability.
      const normalizedUrl = baseUrl.replace(/\/+$/, "");
      const controller = new AbortController();
      const timeout = setTimeout(() => controller.abort(), 10000);

      // Try base URL first, fall back to /models if needed
      let response: Response;
      try {
        response = await fetch(normalizedUrl, {
          method: "HEAD",
          signal: controller.signal,
        });
      } catch {
        // HEAD may be blocked by CORS, retry with GET to /models
        response = await fetch(normalizedUrl + "/models", {
          method: "GET",
          signal: controller.signal,
        });
      }
      clearTimeout(timeout);

      // ANY HTTP response means the server is reachable
      toast.success(`Provider "${providerId}" is reachable`, {
        description: `${normalizedUrl} → HTTP ${response.status}`,
      });
    } catch (error) {
      // Only network errors (timeout, DNS, connection refused) reach here
      const msg = String(error);
      if (msg.includes("abort")) {
        toast.error(`Provider "${providerId}" timed out (10s)`, {
          description: baseUrl,
        });
      } else {
        toast.error(`Provider "${providerId}" is unreachable`, {
          description: msg,
        });
      }
    }
  };

  return (
    <div className="px-6 flex flex-col flex-1 min-h-0 overflow-hidden">
      {/* Provider list - scrollable */}
      <div className="flex-1 overflow-y-auto overflow-x-hidden pb-12 px-1 pt-4">
        <div className="space-y-4">
          <PiProviderList
            providers={providers}
            onEdit={editProvider}
            onDuplicate={duplicateProvider}
            onDelete={(id) => void deleteProviderDirect(id)}
            onTestConnectivity={testConnectivity}
          />
        </div>
      </div>

      {/* FullScreenPanel: Add / Edit */}
      <FullScreenPanel
        isOpen={isEditOpen}
        title={draft.providerId ? `Edit: ${draft.providerId}` : "Add Provider"}
        onClose={() => setIsEditOpen(false)}
        footer={
          <Button
            type="button"
            onClick={() => void buildPreview()}
            disabled={!draft.providerId.trim()}
          >
            Preview & Review
          </Button>
        }
      >
        <PiProviderForm value={draft} onChange={setDraft} />
      </FullScreenPanel>

      {/* FullScreenPanel: Review & Apply */}
      <FullScreenPanel
        isOpen={isReviewOpen}
        title="Review Changes"
        onClose={() => {
          setIsReviewOpen(false);
          setIsEditOpen(true);
        }}
        footer={
          <div className="flex gap-2">
            {draft.providerId.trim() && (
              <Button
                type="button"
                variant="destructive"
                onClick={() => void deletePreview()}
                disabled={isApplying}
              >
                Delete Provider
              </Button>
            )}
            <Button
              type="button"
              onClick={() => void applyPreview()}
              disabled={isApplying}
            >
              {isApplying ? "Applying..." : "Apply to models.json"}
            </Button>
          </div>
        }
      >
        <PiProviderDiffPreview
          preview={preview}
          isApplying={isApplying}
          onApply={() => void applyPreview()}
          onDelete={() => void deletePreview()}
          canDelete={Boolean(draft.providerId.trim())}
        />
      </FullScreenPanel>
    </div>
  );
}
