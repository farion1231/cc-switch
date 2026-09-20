import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type {
  ConfigColumn,
  ConfigSchemaItem,
  PluginConfigDocs,
} from "@/types/plugin";

// 通用插件配置对话框：按 config_schema 渲染表单，读写经插件协议
// （plugin_config_read/write）由插件自行校验与落盘——本组件不含任何插件语义。

const I18N = "settings.advanced.plugins.config";

/** 点路径取值（"hash.algorithm" → doc.hash?.algorithm） */
function getByPath(doc: unknown, path: string): unknown {
  let cur: unknown = doc;
  for (const seg of path.split(".")) {
    if (cur && typeof cur === "object" && seg in (cur as Record<string, unknown>)) {
      cur = (cur as Record<string, unknown>)[seg];
    } else {
      return undefined;
    }
  }
  return cur;
}

/** 点路径设值（沿途补齐中间对象） */
function setByPath(doc: unknown, path: string, value: unknown): void {
  const segs = path.split(".");
  let cur = doc as Record<string, unknown>;
  for (let i = 0; i < segs.length - 1; i++) {
    const seg = segs[i];
    if (!(cur[seg] instanceof Object)) cur[seg] = {};
    cur = cur[seg] as Record<string, unknown>;
  }
  cur[segs[segs.length - 1]] = value;
}

interface Props {
  pluginId: string;
  displayName: string;
  schema: ConfigSchemaItem[];
  open: boolean;
  onClose: () => void;
}

export function PluginConfigDialog({ pluginId, displayName, schema, open, onClose }: Props) {
  const { t } = useTranslation();
  const [docs, setDocs] = useState<PluginConfigDocs | null>(null);
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    setDocs(null);
    try {
      setDocs(await invoke<PluginConfigDocs>("plugin_config_read", { id: pluginId }));
    } catch (e) {
      toast.error(String(e));
      onClose();
    }
  }, [pluginId]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (open) void load();
  }, [open, load]);

  const setDoc = (file: string, next: unknown) => {
    setDocs((prev) => ({ ...(prev ?? {}), [file]: next }));
  };

  const updateValue = (item: ConfigSchemaItem, value: unknown) => {
    if (!docs) return;
    const doc = docs[item.file];
    const next =
      doc instanceof Object ? structuredClone(doc) : {};
    setByPath(next, item.path ?? item.key, value);
    setDoc(item.file, next);
  };

  const updateCell = (
    item: ConfigSchemaItem,
    rowIndex: number,
    column: ConfigColumn,
    value: unknown,
  ) => {
    if (!docs) return;
    const doc = structuredClone(docs[item.file] ?? {});
    const rows = getByPath(doc, item.path ?? item.key);
    if (!Array.isArray(rows)) return;
    rows[rowIndex] = { ...(rows[rowIndex] as Record<string, unknown>), [column.key]: value };
    setDoc(item.file, doc);
  };

  const addRow = (item: ConfigSchemaItem) => {
    if (!docs) return;
    const doc = structuredClone(docs[item.file] ?? {});
    const path = item.path ?? item.key;
    const rows = getByPath(doc, path);
    if (!Array.isArray(rows)) return;
    const blank: Record<string, unknown> = {};
    for (const col of item.columns ?? []) {
      blank[col.key] =
        col.type === "toggle" ? true : col.type === "number" ? 0 : "";
    }
    rows.push(blank);
    setDoc(item.file, doc);
  };

  const delRow = (item: ConfigSchemaItem, rowIndex: number) => {
    if (!docs) return;
    const doc = structuredClone(docs[item.file] ?? {});
    const rows = getByPath(doc, item.path ?? item.key);
    if (!Array.isArray(rows)) return;
    rows.splice(rowIndex, 1);
    setDoc(item.file, doc);
  };

  const save = async () => {
    if (!docs) return;
    setSaving(true);
    try {
      await invoke("plugin_config_write", { id: pluginId, docs });
      toast.success(t(`${I18N}.saved`));
      onClose();
    } catch (e) {
      // 插件侧校验错误原样透传展示
      toast.error(t(`${I18N}.failed`, { error: String(e) }));
    } finally {
      setSaving(false);
    }
  };

  const renderField = (item: ConfigSchemaItem) => {
    const value = getByPath(docs?.[item.file], item.path ?? item.key);
    const field: React.ReactNode[] = [];
    field.push(
      <Label key={`l-${item.key}`} className="text-sm font-medium">
        {item.label}
        {item.type === "select" || item.type === "table" ? null : (
          <span className="ml-2 text-xs text-muted-foreground">
            {item.file} · {item.path ?? item.key}
          </span>
        )}
      </Label>,
    );
    if (item.description) {
      field.push(
        <p key={`d-${item.key}`} className="text-xs text-muted-foreground">
          {item.description}
        </p>,
      );
    }
    switch (item.type) {
      case "toggle":
        field.push(
          <Switch
            key={`f-${item.key}`}
            checked={value === true}
            onCheckedChange={(checked) => updateValue(item, checked)}
          />,
        );
        break;
      case "number":
        field.push(
          <Input
            key={`f-${item.key}`}
            type="number"
            value={typeof value === "number" ? value : ""}
            onChange={(e) => updateValue(item, Number(e.target.value))}
          />,
        );
        break;
      case "select":
        field.push(
          <select
            key={`f-${item.key}`}
            className="h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm"
            value={typeof value === "string" ? value : ""}
            onChange={(e) => updateValue(item, e.target.value)}
          >
            {(item.options ?? []).map((o) => (
              <option key={o} value={o}>
                {o}
              </option>
            ))}
          </select>,
        );
        break;
      case "textarea":
        field.push(
          <textarea
            key={`f-${item.key}`}
            className="min-h-[72px] w-full rounded-md border border-input bg-transparent px-3 py-2 font-mono text-xs"
            value={typeof value === "string" ? value : ""}
            onChange={(e) => updateValue(item, e.target.value)}
          />,
        );
        break;
      case "table": {
        const rows = Array.isArray(value) ? (value as Record<string, unknown>[]) : [];
        const cols = item.columns ?? [];
        field.push(
          <table key={`f-${item.key}`} className="w-full text-xs">
            <thead>
              <tr>
                {cols.map((c) => (
                  <th key={c.key} className="border-b px-2 py-1 text-left font-medium">
                    {c.label}
                  </th>
                ))}
                <th className="w-14 border-b px-2 py-1" />
              </tr>
            </thead>
            <tbody>
              {rows.map((row, ri) => (
                <tr key={ri}>
                  {cols.map((c) => (
                    <td key={c.key} className="px-2 py-1 align-top">
                      {c.type === "toggle" ? (
                        <Switch
                          checked={row[c.key] === true}
                          onCheckedChange={(checked) => updateCell(item, ri, c, checked)}
                        />
                      ) : c.type === "textarea" ? (
                        <textarea
                          className="w-full min-w-[220px] rounded-md border border-input bg-transparent px-2 py-1 font-mono text-xs"
                          rows={2}
                          value={typeof row[c.key] === "string" ? (row[c.key] as string) : ""}
                          onChange={(e) => updateCell(item, ri, c, e.target.value)}
                        />
                      ) : c.type === "number" ? (
                        <Input
                          type="number"
                          className="w-24"
                          value={typeof row[c.key] === "number" ? (row[c.key] as number) : ""}
                          onChange={(e) => updateCell(item, ri, c, Number(e.target.value))}
                        />
                      ) : c.type === "select" ? (
                        <select
                          className="h-8 w-full rounded-md border border-input bg-transparent px-2 text-xs"
                          value={typeof row[c.key] === "string" ? (row[c.key] as string) : ""}
                          onChange={(e) => updateCell(item, ri, c, e.target.value)}
                        >
                          {(c.options ?? []).map((o) => (
                            <option key={o} value={o}>
                              {o}
                            </option>
                          ))}
                        </select>
                      ) : (
                        <Input
                          type="text"
                          value={typeof row[c.key] === "string" ? (row[c.key] as string) : ""}
                          onChange={(e) => updateCell(item, ri, c, e.target.value)}
                        />
                      )}
                    </td>
                  ))}
                  <td className="px-2 py-1">
                    <Button
                      variant="ghost"
                      size="sm"
                      className="h-7 px-2 text-destructive"
                      onClick={() => delRow(item, ri)}
                    >
                      ✕
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>,
        );
        field.push(
          <Button
            key={`a-${item.key}`}
            variant="outline"
            size="sm"
            className="mt-2"
            onClick={() => addRow(item)}
          >
            {t(`${I18N}.addRow`)}
          </Button>,
        );
        break;
      }
    }
    return (
      <div key={`g-${item.key}`} className="space-y-1.5">
        {field}
      </div>
    );
  };

  // 按目标文件分组渲染
  const files = Array.from(new Set(schema.map((s) => s.file)));

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[85vh] max-w-3xl overflow-y-auto">
        <DialogHeader>
          <DialogTitle>
            {t(`${I18N}.title`, { name: displayName })}
          </DialogTitle>
          <DialogDescription>{t(`${I18N}.description`)}</DialogDescription>
        </DialogHeader>
        {docs === null ? (
          <p className="py-6 text-center text-sm text-muted-foreground">
            {t(`${I18N}.loading`)}
          </p>
        ) : (
          <div className="space-y-5">
            {files.map((file) => (
              <div key={file} className="space-y-3 rounded-lg border p-3">
                <p className="font-mono text-xs text-muted-foreground">{file}</p>
                {schema
                  .filter((s) => s.file === file)
                  .map((item) => renderField(item))}
              </div>
            ))}
          </div>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t(`${I18N}.cancel`)}
          </Button>
          <Button onClick={() => void save()} disabled={saving || docs === null}>
            {t(`${I18N}.save`)}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
