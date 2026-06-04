import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Plus, Trash2 } from "lucide-react";

export interface CustomHeadersEditorProps {
  headers: Record<string, string>;
  onChange: (headers: Record<string, string>) => void;
}

export function CustomHeadersEditor({
  headers,
  onChange,
}: CustomHeadersEditorProps) {
  const { t } = useTranslation();
  const entries = Object.entries(headers);

  const handleAdd = () => {
    onChange({ ...headers, "": "" });
  };

  const handleRemove = (index: number) => {
    const newEntries = entries.filter((_, i) => i !== index);
    const newHeaders: Record<string, string> = {};
    for (const [k, v] of newEntries) {
      if (k) newHeaders[k] = v;
    }
    onChange(newHeaders);
  };

  const handleKeyChange = (index: number, newKey: string) => {
    const newEntries = entries.map(([k, v], i) =>
      i === index ? [newKey, v] : [k, v],
    );
    const newHeaders: Record<string, string> = {};
    for (const [k, v] of newEntries) {
      if (k) newHeaders[k] = v;
    }
    onChange(newHeaders);
  };

  const handleValueChange = (index: number, newValue: string) => {
    const newEntries = entries.map(([k, v], i) =>
      i === index ? [k, newValue] : [k, v],
    );
    const newHeaders: Record<string, string> = {};
    for (const [k, v] of newEntries) {
      if (k) newHeaders[k] = v;
    }
    onChange(newHeaders);
  };

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <Label className="text-sm font-medium">
          {t("customHeaders.title", { defaultValue: "自定义请求头" })}
        </Label>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={handleAdd}
          className="h-8 gap-1"
        >
          <Plus className="h-3.5 w-3.5" />
          {t("customHeaders.add", { defaultValue: "添加" })}
        </Button>
      </div>

      {entries.length === 0 && (
        <p className="text-sm text-muted-foreground">
          {t("customHeaders.empty", { defaultValue: "未配置自定义请求头" })}
        </p>
      )}

      {entries.map(([key, value], index) => (
        <div key={index} className="flex items-center gap-2">
          <Input
            placeholder={t("customHeaders.name", {
              defaultValue: "Header 名称",
            })}
            value={key}
            onChange={(e) => handleKeyChange(index, e.target.value)}
            className="flex-1"
          />
          <Input
            placeholder={t("customHeaders.value", {
              defaultValue: "Header 值",
            })}
            value={value}
            onChange={(e) => handleValueChange(index, e.target.value)}
            className="flex-1"
          />
          <Button
            type="button"
            variant="ghost"
            size="icon"
            onClick={() => handleRemove(index)}
            className="h-9 w-9 shrink-0 text-muted-foreground hover:text-destructive"
          >
            <Trash2 className="h-4 w-4" />
          </Button>
        </div>
      ))}
    </div>
  );
}
