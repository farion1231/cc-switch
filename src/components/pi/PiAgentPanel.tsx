import { forwardRef, useEffect, useImperativeHandle, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Save } from "lucide-react";
import { piApi } from "@/lib/api";
import type { PiProviderDraft, PiProvidersMap } from "@/types/pi";
import {
  emptyPiProviderDraft,
  PiProviderForm,
} from "@/components/pi/PiProviderForm";
import { providerToDraft } from "@/components/pi/piProviderMapping";
import { PiProviderList } from "@/components/pi/PiProviderList";
import { Button } from "@/components/ui/button";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { ConfirmDialog } from "@/components/ConfirmDialog";

export interface PiAgentPanelHandle {
  openAdd: () => void;
}

export const PiAgentPanel = forwardRef<PiAgentPanelHandle>((_props, ref) => {
  const { t } = useTranslation();
  const [providers, setProviders] = useState<PiProvidersMap>({});
  const [draft, setDraft] = useState<PiProviderDraft>(emptyPiProviderDraft);
  const [view, setView] = useState<"list" | "edit">("list");
  const [isSaving, setIsSaving] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);

  const refresh = async () => {
    try {
      setProviders(await piApi.listProviders());
    } catch (error) {
      toast.error(t("pi.toast.readFailed"), {
        description: String(error),
      });
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  const startNew = () => {
    setDraft({ ...emptyPiProviderDraft });
    setView("edit");
  };

  useImperativeHandle(ref, () => ({
    openAdd: startNew,
  }));

  const editProvider = (providerId: string) => {
    const provider = providers[providerId] as
      | Record<string, unknown>
      | undefined;
    if (!provider) {
      startNew();
      return;
    }
    // Single source of truth: fully round-trips cost, all compat flags, and
    // advancedJson so editing an existing provider never drops data.
    setDraft(providerToDraft(provider, { providerId }));
    setView("edit");
  };

  const saveProvider = async () => {
    if (!draft.providerId.trim()) {
      toast.error(
        t("pi.save.providerIdRequired", {
          defaultValue: "请填写供应商标识",
        }),
      );
      return;
    }

    setIsSaving(true);
    try {
      const preview = await piApi.previewProviderPatch(draft);
      const result = await piApi.applyProviderPatch(
        draft,
        preview.currentFileHash,
      );
      toast.success(t("pi.toast.saved"), {
        description: t("pi.toast.savedDesc", { path: result.backupPath }),
      });
      await refresh();
      setView("list");
    } catch (error) {
      toast.error(t("pi.toast.applyFailed"), {
        description: String(error),
      });
    } finally {
      setIsSaving(false);
    }
  };

  const confirmDeleteProvider = (providerId: string) => {
    setDeleteTarget(providerId);
  };

  const executeDelete = async () => {
    if (!deleteTarget) return;
    setIsSaving(true);
    try {
      // Delete only needs the current file hash for optimistic locking; it must
      // NOT go through previewProviderPatch, which validates the draft as a
      // custom upsert and would reject the empty delete draft ("baseUrl is
      // required for custom providers") before deleteProvider ever runs.
      const meta = await piApi.readModelsMeta();
      const result = await piApi.deleteProvider(deleteTarget, meta.fileHash);
      toast.success(t("pi.toast.deleted"), {
        description: t("pi.toast.savedDesc", { path: result.backupPath }),
      });
      setDraft({ ...emptyPiProviderDraft });
      await refresh();
    } catch (error) {
      toast.error(t("pi.toast.deleteFailed"), {
        description: String(error),
      });
    } finally {
      setIsSaving(false);
      setDeleteTarget(null);
    }
  };

  const duplicateProvider = (providerId: string) => {
    editProvider(providerId);
    // After editProvider sets the draft, override the providerId to force "new"
    setDraft((prev) => ({
      ...prev,
      providerId: `${prev.providerId}-copy`,
    }));
  };

  const testConnectivity = async (providerId: string) => {
    const provider = providers[providerId] as
      | Record<string, unknown>
      | undefined;
    const baseUrl =
      typeof provider?.baseUrl === "string" ? provider.baseUrl : "";
    const normalizedUrl = baseUrl.replace(/\/+$/, "");

    try {
      const result = await piApi.testConnectivity(providerId);
      if (result.reachable) {
        toast.success(t("pi.toast.reachable", { id: providerId }), {
          description: t("pi.toast.reachableDesc", {
            url: normalizedUrl,
            status: result.statusCode ?? 0,
          }),
        });
      } else if (result.errorKind === "noBaseUrl") {
        toast.error(t("pi.toast.noBaseUrl"));
      } else if (result.errorKind === "timeout") {
        toast.error(t("pi.toast.timeout", { id: providerId }), {
          description: baseUrl,
        });
      } else {
        toast.error(t("pi.toast.unreachable", { id: providerId }), {
          description: result.detail ?? "",
        });
      }
    } catch (error) {
      toast.error(t("pi.toast.unreachable", { id: providerId }), {
        description: String(error),
      });
    }
  };

  return (
    <div className="px-6 pt-4 pb-12">
      {view === "list" ? (
        <PiProviderList
          providers={providers}
          onEdit={editProvider}
          onDuplicate={duplicateProvider}
          onDelete={confirmDeleteProvider}
          onTestConnectivity={testConnectivity}
        />
      ) : (
        <FullScreenPanel
          isOpen={view === "edit"}
          title={t("pi.editor.title", { defaultValue: "编辑供应商" })}
          onClose={() => setView("list")}
          footer={
            <Button
              type="button"
              onClick={() => void saveProvider()}
              disabled={isSaving}
              className="bg-primary text-primary-foreground hover:bg-primary/90"
            >
              <Save className="h-4 w-4 mr-2" />
              {t("common.save")}
            </Button>
          }
        >
          <div className="max-w-4xl mx-auto">
            <PiProviderForm value={draft} onChange={setDraft} />
          </div>
        </FullScreenPanel>
      )}

      <ConfirmDialog
        isOpen={deleteTarget !== null}
        title={t("pi.deleteConfirm.title", { defaultValue: "删除供应商" })}
        message={t("pi.deleteConfirm.message", {
          id: deleteTarget ?? "",
          defaultValue: `确定要删除供应商 "${deleteTarget}" 吗？此操作不可撤销。`,
        })}
        confirmText={t("common.delete")}
        variant="destructive"
        onConfirm={() => void executeDelete()}
        onCancel={() => setDeleteTarget(null)}
      />
    </div>
  );
});

PiAgentPanel.displayName = "PiAgentPanel";
