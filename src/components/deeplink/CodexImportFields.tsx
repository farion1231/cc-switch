import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import type { DeepLinkImportRequest } from "@/lib/api/deeplink";
import { reasoningLevels, updateImportModel } from "@/utils/codexImport";

export function CodexImportFields({
  request,
  onChange,
}: {
  request: DeepLinkImportRequest;
  onChange: (request: DeepLinkImportRequest) => void;
}) {
  const rows = request.modelCatalog?.models ?? [];
  return (
    <section
      className="space-y-3 rounded-md border p-4"
      aria-label="Codex 导入自定义"
    >
      <p className="font-medium">Codex 固定导入模板</p>
      <p className="text-sm text-muted-foreground">
        每次新增一份，保留旧配置。补齐模型及支持的档位后保存；稍后手动启用时选择覆盖项。
      </p>
      <label className="block text-sm">
        API 地址
        <Input
          value={request.endpoint ?? ""}
          onChange={(e) => onChange({ ...request, endpoint: e.target.value })}
        />
      </label>
      <label className="block text-sm">
        API Key
        <Input
          type="password"
          autoComplete="off"
          value={request.apiKey ?? ""}
          onChange={(e) => onChange({ ...request, apiKey: e.target.value })}
        />
      </label>
      {rows.map((row, index) => (
        <div key={index} className="space-y-2 rounded border p-3">
          <div className="flex gap-2">
            <Input
              aria-label={`模型 ${index + 1}`}
              placeholder="模型 ID"
              value={row.model}
              onChange={(e) =>
                onChange(
                  updateImportModel(request, index, {
                    ...row,
                    model: e.target.value,
                  }),
                )
              }
            />
            <Button
              type="button"
              variant="outline"
              onClick={() =>
                onChange({
                  ...request,
                  modelCatalog: {
                    ...request.modelCatalog,
                    models: rows.filter((_, i) => i !== index),
                  },
                })
              }
            >
              移除
            </Button>
          </div>
          <p className="text-xs text-muted-foreground">
            建议仅勾选此服务商明确支持的档位。未声明的档位需要你确认。
          </p>
          <div className="flex flex-wrap gap-3">
            {reasoningLevels.map((level) => (
              <label key={level} className="flex items-center gap-1 text-sm">
                <input
                  type="checkbox"
                  checked={row.reasoningLevels?.includes(level) ?? false}
                  onChange={(e) =>
                    onChange(
                      updateImportModel(request, index, {
                        ...row,
                        reasoningLevels: e.target.checked
                          ? [...(row.reasoningLevels ?? []), level]
                          : row.reasoningLevels?.filter((v) => v !== level),
                      }),
                    )
                  }
                />
                {level}
              </label>
            ))}
          </div>
        </div>
      ))}
      <Button
        type="button"
        variant="outline"
        onClick={() =>
          onChange({
            ...request,
            modelCatalog: {
              ...request.modelCatalog,
              models: [...rows, { model: "" }],
            },
          })
        }
      >
        添加模型
      </Button>
      <label className="block text-sm">
        默认模型
        <select
          className="mt-1 w-full rounded border bg-background p-2"
          value={request.model ?? ""}
          onChange={(e) => onChange({ ...request, model: e.target.value })}
        >
          <option value="">请选择（采用导入值，缺失时补填）</option>
          {rows
            .filter((row) => row.model)
            .map((row, i) => (
              <option key={i} value={row.model}>
                {row.model}
              </option>
            ))}
        </select>
      </label>
      <p className="text-xs text-muted-foreground">
        余额脚本：导入内容优先，缺失时使用固定模板。脚本正文在下方预览，保存后仍可编辑。
      </p>
    </section>
  );
}
