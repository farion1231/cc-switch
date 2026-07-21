import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Edit2, Trash2, Globe, Power, PowerOff } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ProviderIcon } from "@/components/ProviderIcon";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { RouteEntryForm } from "./RouteEntryForm";
import type { UpstreamRoute } from "@/types";

interface RouteTableEditorProps {
  routes: UpstreamRoute[];
  onChange: (routes: UpstreamRoute[]) => void;
}

export function RouteTableEditor({ routes, onChange }: RouteTableEditorProps) {
  const { t } = useTranslation();
  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<UpstreamRoute | null>(null);
  const [deleteId, setDeleteId] = useState<string | null>(null);

  const handleSave = (route: UpstreamRoute) => {
    if (editing) {
      onChange(routes.map((r) => (r.id === route.id ? route : r)));
    } else {
      onChange([...routes, route]);
    }
    setEditing(null);
  };

  const handleDelete = () => {
    if (deleteId) {
      onChange(routes.filter((r) => r.id !== deleteId));
      setDeleteId(null);
    }
  };

  const toggleEnabled = (id: string) => {
    onChange(
      routes.map((r) => (r.id === id ? { ...r, enabled: !r.enabled } : r)),
    );
  };

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <p className="text-sm font-medium">路由表</p>
        <Button
          size="sm"
          variant="outline"
          className="h-7 text-xs"
          onClick={() => {
            setEditing(null);
            setFormOpen(true);
          }}
        >
          <Plus className="mr-1 h-3.5 w-3.5" />
          添加路由目标
        </Button>
      </div>

      <p className="text-xs text-muted-foreground">
        配置上游供应商，代理按 model 字段自动匹配并转发
      </p>

      {routes.length === 0 ? (
        <div className="flex flex-col items-center justify-center rounded-lg border border-dashed py-6 text-center">
          <Globe className="mb-2 h-8 w-8 text-muted-foreground/40" />
          <p className="text-xs text-muted-foreground">
            暂无路由目标，点击上方按钮添加
          </p>
        </div>
      ) : (
        <div className="grid gap-3 sm:grid-cols-2">
          {routes.map((route) => (
            <div
              key={route.id}
              className={`relative rounded-xl border p-4 transition-all hover:shadow-sm ${
                route.enabled
                  ? "border-border/50 bg-card hover:border-border"
                  : "border-dashed border-muted bg-muted/30 opacity-60"
              }`}
            >
              <div className="flex items-start justify-between">
                <div className="flex items-center gap-3 min-w-0 flex-1">
                  <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-accent">
                    <ProviderIcon name={route.name} size={20} />
                  </div>
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="truncate text-sm font-medium">
                        {route.name}
                      </span>
                      <button
                        onClick={() => toggleEnabled(route.id)}
                        className="shrink-0"
                        title={route.enabled ? "已启用" : "已禁用"}
                      >
                        {route.enabled ? (
                          <Power className="h-3 w-3 text-green-500" />
                        ) : (
                          <PowerOff className="h-3 w-3 text-muted-foreground" />
                        )}
                      </button>
                    </div>
                    <p className="mt-0.5 truncate text-xs text-muted-foreground">
                      {route.baseUrl}
                    </p>
                    <div className="mt-1 flex items-center gap-2">
                      <span className="rounded bg-accent px-1.5 py-0.5 text-[10px] font-medium uppercase text-muted-foreground">
                        {route.protocol || "—"}
                      </span>
                      <span className="text-[10px] text-muted-foreground">
                        {route.modelNames?.length || 0} 个模型
                      </span>
                    </div>
                  </div>
                </div>

                <div className="flex items-center gap-1">
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7"
                    onClick={() => {
                      setEditing(route);
                      setFormOpen(true);
                    }}
                  >
                    <Edit2 className="h-3.5 w-3.5" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7 text-destructive"
                    onClick={() => setDeleteId(route.id)}
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </Button>
                </div>
              </div>
            </div>
          ))}
        </div>
      )}

      <RouteEntryForm
        isOpen={formOpen}
        onClose={() => {
          setFormOpen(false);
          setEditing(null);
        }}
        onSave={handleSave}
        editingRoute={editing}
      />

      <ConfirmDialog
        isOpen={!!deleteId}
        title="删除路由目标"
        message="确定要删除此路由目标吗？"
        confirmText={t("common.delete", { defaultValue: "删除" })}
        onConfirm={handleDelete}
        onCancel={() => setDeleteId(null)}
      />
    </div>
  );
}
