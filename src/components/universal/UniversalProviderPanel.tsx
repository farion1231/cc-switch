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
          defaultValue: "加载统一供应商失败",
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
                defaultValue: "统一供应商已更新",
              })
            : t("universalProvider.addedAndSynced", {
                defaultValue: "统一供应商已添加并同步",
              }),
        );
        loadProviders();
        setEditingProvider(null);
      } catch (error) {
        console.error("Failed to save universal provider:", error);
        toast.error(
          t("universalProvider.saveError", {
            defaultValue: "保存统一供应商失败",
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
            defaultValue: "已保存并同步到所有应用",
          }),
        );
        loadProviders();
        setEditingProvider(null);
      } catch (error) {
        console.error("Failed to save and sync universal provider:", error);
        toast.error(
          t("universalProvider.saveAndSyncError", {
            defaultValue: "保存并同步失败",
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
        t("universalProvider.deleted", { defaultValue: "统一供应商已删除" }),
      );
      loadProviders();
    } catch (error) {
      console.error("Failed to delete universal provider:", error);
      toast.error(
        t("universalProvider.deleteError", {
          defaultValue: "删除统一供应商失败",
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
        t("universalProvider.synced", { defaultValue: "已同步到所有应用" }),
      );
    } catch (error) {
      console.error("Failed to sync universal provider:", error);
      toast.error(
        t("universalProvider.syncError", {
          defaultValue: "同步统一供应商失败",
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
          {t("universalProvider.title", { defaultValue: "统一供应商" })}
        </h2>
        <span className="rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">
          {providerList.length}
        </span>
      </div>

      {/* 描述 */}
      <p className="text-sm text-muted-foreground">
        {t("universalProvider.description", {
          defaultValue:
            "统一供应商可以同时管理 Claude、Codex 和 Gemini 的配置。修改后会自动同步到所有启用的应用。",
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
              defaultValue: "还没有统一供应商",
            })}
          </p>
          <p className="mt-1 text-xs text-muted-foreground/70">
            {t("universalProvider.emptyHint", {
              defaultValue: "点击下方「添加统一供应商」按钮创建一个",
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
          defaultValue: "删除统一供应商",
        })}
        message={t("universalProvider.deleteConfirmDescription", {
          defaultValue: `确定要删除 "${deleteConfirm.name}" 吗？这将同时删除它在各应用中生成的供应商配置。`,
          name: deleteConfirm.name,
        })}
        confirmText={t("common.delete", { defaultValue: "删除" })}
        onConfirm={handleDelete}
        onCancel={() => setDeleteConfirm({ open: false, id: "", name: "" })}
      />

      {/* 同步确认对话框 */}
      <ConfirmDialog
        isOpen={syncConfirm.open}
        title={t("universalProvider.syncConfirmTitle", {
          defaultValue: "同步统一供应商",
        })}
        message={t("universalProvider.syncConfirmDescription", {
          defaultValue: `同步 "${syncConfirm.name}" 将会覆盖 Claude、Codex 和 Gemini 中关联的供应商配置。确定要继续吗？`,
          name: syncConfirm.name,
        })}
        confirmText={t("universalProvider.syncConfirm", {
          defaultValue: "同步",
        })}
        onConfirm={handleSync}
        onCancel={() => setSyncConfirm({ open: false, id: "", name: "" })}
      />
    </div>
  );
}
