import { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { Cable, Pencil, Trash2 } from "lucide-react";
import { toast } from "@/lib/toast";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { endpointsApi } from "@/lib/api";
import type { Endpoint, EndpointsMap, EndpointReference } from "@/types";
import { EndpointFormModal } from "./EndpointFormModal";

interface EndpointPanelProps {
  onRefChanged?: () => void;
  /** 嵌入模式（授权中心等）：隐藏自带标题与描述，只留列表与操作 */
  embedded?: boolean;
}

/**
 * 全局端点管理面板：厂商连接信息的单一事实源。
 *
 * 端点只管"连到哪家"（base_url / api_key / wire 格式）；
 * 分组条目经 endpointRef 引用并各自配策略与档位映射。
 * 编辑端点凭据会级联重物化所有引用条目——改一处全局生效。
 */
export function EndpointPanel({ onRefChanged, embedded }: EndpointPanelProps) {
  const { t } = useTranslation();

  const [endpoints, setEndpoints] = useState<EndpointsMap>({});
  const [loading, setLoading] = useState(true);
  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<Endpoint | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<{
    open: boolean;
    endpoint: Endpoint | null;
    refs: EndpointReference[];
  }>({ open: false, endpoint: null, refs: [] });

  const load = useCallback(async () => {
    try {
      setLoading(true);
      setEndpoints(await endpointsApi.getAll());
    } catch (error) {
      console.error("Failed to load endpoints:", error);
      toast.error(t("endpoints.loadError", { defaultValue: "加载端点失败" }));
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    load();
  }, [load]);

  const handleSave = useCallback(
    async (endpoint: Endpoint) => {
      try {
        await endpointsApi.save(endpoint);
        toast.success(
          t("endpoints.saved", {
            defaultValue: "端点已保存，引用它的条目已同步更新（如有）",
          }),
        );
        await load();
        onRefChanged?.();
      } catch (error) {
        console.error("Failed to save endpoint:", error);
        toast.error(t("endpoints.saveError", { defaultValue: "保存端点失败" }));
      }
    },
    [load, onRefChanged, t],
  );

  const handleDeleteClick = useCallback(
    async (endpoint: Endpoint) => {
      try {
        await endpointsApi.delete(endpoint.id);
        setDeleteTarget({ open: false, endpoint: null, refs: [] });
        toast.success(t("endpoints.deleted", { defaultValue: "端点已删除" }));
        await load();
      } catch (error) {
        const refs = (error as { refs?: EndpointReference[] }).refs;
        if (refs && refs.length > 0) {
          setDeleteTarget({ open: true, endpoint, refs });
          return;
        }
        console.error("Failed to delete endpoint:", error);
        toast.error(
          t("endpoints.deleteError", { defaultValue: "删除端点失败" }),
        );
      }
    },
    [load, t],
  );

  const list = Object.values(endpoints);

  return (
    <div className="space-y-4">
      {!embedded && (
        <>
          <div className="flex items-center gap-2">
            <Cable className="h-5 w-5 text-primary" />
            <h2 className="text-lg font-semibold">
              {t("endpoints.title", { defaultValue: "全局端点" })}
            </h2>
            <span className="rounded-full bg-subtle px-2 py-0.5 text-xs text-fg-2">
              {list.length}
            </span>
          </div>

          <p className="text-sm text-fg-2">
            {t("endpoints.description", {
              defaultValue:
                "端点保存厂商的连接信息（地址、密钥、协议格式），各分组的供应商条目引用它。在这里改一次密钥，所有引用条目同时生效。",
            })}
          </p>
        </>
      )}

      {loading ? (
        <div className="flex items-center justify-center py-12">
          <div className="h-6 w-6 animate-spin rounded-full border-2 border-primary border-t-transparent" />
        </div>
      ) : list.length === 0 ? (
        <div className="flex flex-col items-center justify-center rounded-xl border border-dashed py-12 text-center">
          <Cable className="mb-3 h-10 w-10 text-fg-3" />
          <p className="text-sm text-fg-2">
            {t("endpoints.empty", { defaultValue: "还没有端点" })}
          </p>
          <p className="mt-1 text-xs text-fg-3">
            {t("endpoints.emptyHint", {
              defaultValue:
                "从供应商条目「引用端点」时会自动创建，或点下方按钮新建",
            })}
          </p>
        </div>
      ) : (
        <div className="space-y-2">
          {list.map((endpoint) => (
            <div
              key={endpoint.id}
              className="flex items-center justify-between gap-3 rounded-lg border px-4 py-3"
            >
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="truncate font-medium">{endpoint.name}</span>
                  {endpoint.apiFormat && endpoint.apiFormat !== "anthropic" && (
                    <span className="rounded bg-subtle px-1.5 py-0.5 text-[10px] uppercase text-fg-2">
                      {endpoint.apiFormat}
                    </span>
                  )}
                </div>
                <div className="mt-0.5 truncate text-xs text-fg-3">
                  {endpoint.baseUrl}
                </div>
              </div>
              <div className="flex shrink-0 items-center gap-1">
                <Button
                  variant="ghost"
                  size="icon"
                  onClick={() => {
                    setEditing(endpoint);
                    setFormOpen(true);
                  }}
                  title={t("common.edit", { defaultValue: "编辑" })}
                >
                  <Pencil className="h-4 w-4" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  onClick={() => handleDeleteClick(endpoint)}
                  title={t("common.delete", { defaultValue: "删除" })}
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}

      <Button
        onClick={() => {
          setEditing(null);
          setFormOpen(true);
        }}
        className="w-full"
      >
        {t("endpoints.add", { defaultValue: "新建端点" })}
      </Button>

      <EndpointFormModal
        isOpen={formOpen}
        onClose={() => {
          setFormOpen(false);
          setEditing(null);
        }}
        onSave={handleSave}
        editing={editing}
      />

      <ConfirmDialog
        isOpen={deleteTarget.open}
        title={t("endpoints.deleteConfirmTitle", {
          defaultValue: "无法删除：端点正在被引用",
        })}
        message={t("endpoints.deleteConfirmDescription", {
          defaultValue: `「${deleteTarget.endpoint?.name ?? ""}」仍被 ${deleteTarget.refs.length} 个供应商条目引用（${deleteTarget.refs.map(([app]) => app).join("、")}）。请先在这些条目里改用其他端点或改为独立配置。`,
          count: deleteTarget.refs.length,
          apps: deleteTarget.refs.map(([app]) => app).join("、"),
          name: deleteTarget.endpoint?.name ?? "",
        })}
        confirmText={t("common.gotIt", { defaultValue: "知道了" })}
        onConfirm={() =>
          setDeleteTarget({ open: false, endpoint: null, refs: [] })
        }
        onCancel={() =>
          setDeleteTarget({ open: false, endpoint: null, refs: [] })
        }
      />
    </div>
  );
}
