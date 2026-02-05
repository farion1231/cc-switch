import { useState, useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Layers } from "lucide-react";
import { toast } from "sonner";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { UniversalProviderCard } from "./UniversalProviderCard";
import { UniversalProviderFormModal } from "./UniversalProviderFormModal";
import { universalProvidersApi } from "@/lib/api";
import type { UniversalProvider, UniversalProvidersMap } from "@/types";

export function UniversalProviderPanel() {
  const { t } = useTranslation();

  // 状态
  const [providers, setProviders] = useState<UniversalProvidersMap>({});
  const [loading, setLoading] = useState(true);
  const [isFormOpen, setIsFormOpen] = useState(false);
  const [editingProvider, setEditingProvider] =
    useState<UniversalProvider | null>(null);
  const [deleteConfirm, setDeleteConfirm] = useState<{
    open: boolean;
    id: string;
    name: string;
  }>({ open: false, id: "", name: "" });
  const [syncConfirm, setSyncConfirm] = useState<{
    open: boolean;
    id: string;
    name: string;
  }>({ open: false, id: "", name: "" });

  // 加载数据
  const loadProviders = useCallback(async () => {
    try {
      setLoading(true);
      const data = await universalProvidersApi.getAll();
      setProviders(data);
    } catch (error) {
      console.error("Failed to load universal providers:", error);
      toast.error(
        t("universalProvider.loadError", {
          defaultValue: "Failed to load universal providers",
        }),
      );
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    loadProviders();
  }, [loadProviders]);

  // 添加/编辑供应商
  const handleSave = useCallback(
    async (provider: UniversalProvider) => {
      try {
        await universalProvidersApi.upsert(provider);

        // 新建模式下自动同步到各应用
        if (!editingProvider) {
          await universalProvidersApi.sync(provider.id);
        }

        toast.success(
          editingProvider
            ? t("universalProvider.updated", {
                defaultValue: "Universal provider updated",
              })
            : t("universalProvider.addedAndSynced", {
                defaultValue: "Universal provider added and synced",
              }),
        );
        loadProviders();
        setEditingProvider(null);
      } catch (error) {
        console.error("Failed to save universal provider:", error);
        toast.error(
          t("universalProvider.saveError", {
            defaultValue: "Failed to save universal provider",
          }),
        );
      }
    },
    [editingProvider, loadProviders, t],
  );

  // 保存并同步供应商
  const handleSaveAndSync = useCallback(
    async (provider: UniversalProvider) => {
      try {
        await universalProvidersApi.upsert(provider);
        await universalProvidersApi.sync(provider.id);
        toast.success(
          t("universalProvider.savedAndSynced", {
            defaultValue: "Saved and synced to all apps",
          }),
        );
        loadProviders();
        setEditingProvider(null);
      } catch (error) {
        console.error("Failed to save and sync universal provider:", error);
        toast.error(
          t("universalProvider.saveAndSyncError", {
            defaultValue: "Failed to save and sync",
          }),
        );
      }
    },
    [loadProviders, t],
  );

  // 删除供应商
  const handleDelete = useCallback(async () => {
    if (!deleteConfirm.id) return;

    try {
      await universalProvidersApi.delete(deleteConfirm.id);
      toast.success(
        t("universalProvider.deleted", {
          defaultValue: "Universal provider deleted",
        }),
      );
      loadProviders();
    } catch (error) {
      console.error("Failed to delete universal provider:", error);
      toast.error(
        t("universalProvider.deleteError", {
          defaultValue: "Failed to delete universal provider",
        }),
      );
    } finally {
      setDeleteConfirm({ open: false, id: "", name: "" });
    }
  }, [deleteConfirm.id, loadProviders, t]);

  // 同步供应商
  const handleSync = useCallback(async () => {
    if (!syncConfirm.id) return;

    try {
      await universalProvidersApi.sync(syncConfirm.id);
      toast.success(
        t("universalProvider.synced", { defaultValue: "Synced to all apps" }),
      );
    } catch (error) {
      console.error("Failed to sync universal provider:", error);
      toast.error(
        t("universalProvider.syncError", {
          defaultValue: "Failed to sync universal provider",
        }),
      );
    } finally {
      setSyncConfirm({ open: false, id: "", name: "" });
    }
  }, [syncConfirm.id, t]);

  // 打开同步确认
  const handleSyncClick = useCallback(
    (id: string) => {
      const provider = providers[id];
      setSyncConfirm({
        open: true,
        id,
        name: provider?.name || id,
      });
    },
    [providers],
  );

  // 打开编辑
  const handleEdit = useCallback((provider: UniversalProvider) => {
    setEditingProvider(provider);
    setIsFormOpen(true);
  }, []);

  // 打开删除确认
  const handleDeleteClick = useCallback(
    (id: string) => {
      const provider = providers[id];
      setDeleteConfirm({
        open: true,
        id,
        name: provider?.name || id,
      });
    },
    [providers],
  );

  const providerList = Object.values(providers);

  return (
    <div className="space-y-4">
      {/* 头部 */}
      <div className="flex items-center gap-2">
        <Layers className="h-5 w-5 text-primary" />
        <h2 className="text-lg font-semibold">
          {t("universalProvider.title", { defaultValue: "Universal Provider" })}
        </h2>
        <span className="rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">
          {providerList.length}
        </span>
      </div>

      {/* 描述 */}
      <p className="text-sm text-muted-foreground">
        {t("universalProvider.description", {
          defaultValue:
            "Universal providers can manage configurations for Claude, Codex, and Gemini simultaneously. Changes will be automatically synced to all enabled apps.",
        })}
      </p>

      {/* 供应商列表 */}
      {loading ? (
        <div className="flex items-center justify-center py-12">
          <div className="h-6 w-6 animate-spin rounded-full border-2 border-primary border-t-transparent" />
        </div>
      ) : providerList.length === 0 ? (
        <div className="flex flex-col items-center justify-center rounded-xl border border-dashed py-12 text-center">
          <Layers className="mb-3 h-10 w-10 text-muted-foreground/50" />
          <p className="text-sm text-muted-foreground">
            {t("universalProvider.empty", {
              defaultValue: "No universal providers yet",
            })}
          </p>
          <p className="mt-1 text-xs text-muted-foreground/70">
            {t("universalProvider.emptyHint", {
              defaultValue:
                "Click the button below to add a universal provider",
            })}
          </p>
        </div>
      ) : (
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {providerList.map((provider) => (
            <UniversalProviderCard
              key={provider.id}
              provider={provider}
              onEdit={handleEdit}
              onDelete={handleDeleteClick}
              onSync={handleSyncClick}
            />
          ))}
        </div>
      )}

      {/* 表单模态框 */}
      <UniversalProviderFormModal
        isOpen={isFormOpen}
        onClose={() => {
          setIsFormOpen(false);
          setEditingProvider(null);
        }}
        onSave={handleSave}
        onSaveAndSync={handleSaveAndSync}
        editingProvider={editingProvider}
      />

      {/* 删除确认对话框 */}
      <ConfirmDialog
        isOpen={deleteConfirm.open}
        title={t("universalProvider.deleteConfirmTitle", {
          defaultValue: "Delete Universal Provider",
        })}
        message={t("universalProvider.deleteConfirmDescription", {
          defaultValue: `Are you sure you want to delete "\${deleteConfirm.name}"? This will also delete the provider configurations generated in each app.`,
          name: deleteConfirm.name,
        })}
        confirmText={t("common.delete", { defaultValue: "Delete" })}
        onConfirm={handleDelete}
        onCancel={() => setDeleteConfirm({ open: false, id: "", name: "" })}
      />

      {/* 同步确认对话框 */}
      <ConfirmDialog
        isOpen={syncConfirm.open}
        title={t("universalProvider.syncConfirmTitle", {
          defaultValue: "Sync Universal Provider",
        })}
        message={t("universalProvider.syncConfirmDescription", {
          defaultValue: `Syncing "\${syncConfirm.name}" will overwrite the associated provider configurations in Claude, Codex, and Gemini. Are you sure you want to continue?`,
          name: syncConfirm.name,
        })}
        confirmText={t("universalProvider.syncConfirm", {
          defaultValue: "Sync",
        })}
        onConfirm={handleSync}
        onCancel={() => setSyncConfirm({ open: false, id: "", name: "" })}
      />
    </div>
  );
}
