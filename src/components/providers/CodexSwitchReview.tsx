import { useEffect, useRef, useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import {
  codexSelectionApi,
  type CodexSelection,
  type CodexSwitchPreview,
} from "@/lib/api/codexSelection";

export function CodexSwitchReview({
  preview,
  onClose,
}: {
  preview: CodexSwitchPreview;
  onClose: (selection: CodexSelection | null) => void;
}) {
  const [selected, setSelected] = useState(() =>
    preview.fields.filter((f) => f.selected).map((f) => f.id),
  );
  const [model, setModel] = useState(preview.defaultModel ?? "");
  const [effort, setEffort] = useState(preview.currentEffort ?? "");
  const effectiveModel = selected.includes('["model"]')
    ? model
    : (preview.currentModel ?? "");
  const models = selected.includes("catalog")
    ? preview.models
    : preview.currentModels;
  const levels =
    models.find((m) => m.model === effectiveModel)?.reasoningLevels ?? [];
  const changesModels = selected.some((id) =>
    ["connection", "catalog", '["model"]'].includes(id),
  );
  const valid =
    (!preview.exitToDirect || selected.includes("connection")) &&
    (!changesModels ||
      preview.proxy ||
      preview.native ||
      (!!effectiveModel && levels.includes(effort)));
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose(null);
      }}
    >
      <DialogContent
        zIndex="top"
        className="max-w-3xl max-h-[90vh] overflow-y-auto"
      >
        <DialogHeader>
          <DialogTitle>启用前选择覆盖项</DialogTitle>
          <DialogDescription>
            {preview.proxy
              ? "当前处于代理模式，下面确认代理路由。模型聚合与客户端配置继续由 v4.0.17 的路由契约管理。"
              : preview.native
                ? "官方账号使用 v4.0.17 的账号切换流程。定制导入规则用于第三方服务商。"
                : preview.exitToDirect
                  ? "确认后同时回到直连并应用所选字段。连接信息必须勾选；取消时配置和模式均保持原样。"
                  : preview.routing
                    ? "当前为代理 / 聚合模式。连接项控制代理路由，模型目录包含聚合模型；其他字段默认保留。"
                    : "建议覆盖连接、模型目录和导入的默认模型；其他字段默认保留。未勾选连接信息时，仅应用所选配置，当前服务商保持不变。"}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          {preview.fields.map((field) => (
            <div key={field.id} className="rounded border p-3">
              <label className="flex items-center gap-2 font-medium">
                <input
                  type="checkbox"
                  checked={selected.includes(field.id)}
                  onChange={(e) =>
                    setSelected((old) =>
                      e.target.checked
                        ? [...old, field.id]
                        : old.filter((id) => id !== field.id),
                    )
                  }
                />
                {field.label}
              </label>
              <div className="mt-2 grid grid-cols-2 gap-2 text-xs">
                <div>
                  <span className="text-muted-foreground">现有值</span>
                  <pre className="whitespace-pre-wrap break-all">
                    {field.before || "（未设置）"}
                  </pre>
                </div>
                <div>
                  <span className="text-muted-foreground">导入 / 目标值</span>
                  <pre className="whitespace-pre-wrap break-all">
                    {field.after || "（未设置）"}
                  </pre>
                </div>
              </div>
            </div>
          ))}
          {!preview.proxy && !preview.native && changesModels && (
            <>
              <label className="block text-sm">
                默认模型
                <select
                  aria-label="默认模型"
                  className="mt-1 w-full rounded border bg-background p-2"
                  disabled={!selected.includes('["model"]')}
                  value={effectiveModel}
                  onChange={(e) => setModel(e.target.value)}
                >
                  <option value="">请选择默认模型</option>
                  {models.map((row) => (
                    <option key={row.model} value={row.model}>
                      {row.model}
                    </option>
                  ))}
                </select>
              </label>
              <label className="block text-sm">
                默认思考等级（原值：{preview.currentEffort ?? "未设置"}）
                <select
                  aria-label="默认思考等级"
                  className="mt-1 w-full rounded border bg-background p-2"
                  value={levels.includes(effort) ? effort : ""}
                  onChange={(e) => setEffort(e.target.value)}
                >
                  <option value="">原值不适用或未设置，请选择</option>
                  {levels.map((level) => (
                    <option key={level} value={level}>
                      {level}
                    </option>
                  ))}
                </select>
              </label>
              {!valid && (
                <p role="alert" className="text-sm text-warning-text">
                  请确认有效的模型目录、默认模型及思考等级。未声明的档位请先在供应商编辑器中补全。
                </p>
              )}
            </>
          )}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onClose(null)}>
            取消
          </Button>
          <Button
            disabled={!valid || !selected.length}
            onClick={() =>
              onClose({
                revision: preview.revision,
                selected,
                model: effectiveModel || null,
                effort: effort || null,
                ...(preview.exitToDirect ? { exitToDirect: true } : {}),
              })
            }
          >
            确认应用所选项
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function useCodexSwitchReview(app: string) {
  const [preview, setPreview] = useState<CodexSwitchPreview | null>(null);
  const pending = useRef<((value: CodexSelection | null) => void) | null>(null);
  const generation = useRef(0);
  const close = (value: CodexSelection | null) => {
    pending.current?.(value);
    pending.current = null;
    setPreview(null);
  };
  useEffect(() => {
    generation.current++;
    close(null);
    return () => {
      generation.current++;
      pending.current?.(null);
      pending.current = null;
    };
  }, [app]);
  const review = async (id: string, exitToDirect = false) => {
    const turn = ++generation.current;
    close(null);
    const next = await codexSelectionApi.preview(id, exitToDirect);
    if (generation.current !== turn) return null;
    return new Promise<CodexSelection | null>((resolve) => {
      pending.current = resolve;
      setPreview(next);
    });
  };
  return {
    review,
    dialog: preview ? (
      <CodexSwitchReview
        key={preview.revision}
        preview={preview}
        onClose={close}
      />
    ) : null,
  };
}
