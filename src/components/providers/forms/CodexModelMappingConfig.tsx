import { useTranslation } from "react-i18next";
import { useState, useEffect, useRef } from "react";
import { ChevronDown, ChevronRight, ArrowRightLeft, Plus, Trash2 } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { cn } from "@/lib/utils";

/** Codex 模型映射配置 */
export interface CodexModelMappingConfig {
  enabled: boolean;
  /** 简单模型映射：请求模型 → 目标模型 */
  modelMap: Record<string, string>;
  /** Effort 组合映射：模型ID@effort → 目标模型 */
  effortMap: Record<string, string>;
}

/** 默认配置 */
export const defaultCodexModelMappingConfig: CodexModelMappingConfig = {
  enabled: false,
  modelMap: {},
  effortMap: {},
};

interface CodexModelMappingConfigProps {
  config: CodexModelMappingConfig;
  onConfigChange: (config: CodexModelMappingConfig) => void;
}

/** effort 选项 */
const EFFORT_OPTIONS = ["low", "medium", "high", "xhigh"];

/** 简单映射行类型 */
interface SimpleMappingRow {
  id: string;
  source: string;
  target: string;
  isNew: boolean;
}

/** Effort 映射行类型 */
interface EffortMappingRow {
  id: string;
  model: string;
  effort: string;
  target: string;
  isNew: boolean;
}

export function CodexModelMappingConfig({
  config,
  onConfigChange,
}: CodexModelMappingConfigProps) {
  const { t } = useTranslation();
  const [isOpen, setIsOpen] = useState(config.enabled);

  // 将 config 转换为行数据
  const [simpleRows, setSimpleRows] = useState<SimpleMappingRow[]>([]);
  const [effortRows, setEffortRows] = useState<EffortMappingRow[]>([]);

  // Refs for auto-focus
  const simpleInputRefs = useRef<Map<string, HTMLInputElement>>(new Map());
  const effortInputRefs = useRef<Map<string, HTMLInputElement>>(new Map());

  // 从 config 初始化行数据
  // 从 config 初始化 Simple 行数据
  useEffect(() => {
    const existingSimple: SimpleMappingRow[] = Object.entries(config.modelMap).map(
      ([source, target]) => ({
        id: `simple-${source}`,
        source,
        target,
        isNew: false,
      })
    );
    // 只有当本地没有正在编辑的新行时，或者 config 确实发生了变化（这里简单处理，直接覆盖，但移除了自动添加空行的逻辑）
    // 为了防止正在输入的行被 config 更新覆盖（如果 config 没变），我们理想情况应该做比较。
    // 但鉴于目前逻辑是：只有 valid 这里才会 update config，所以 config 中永远只有 valid rows。
    // 如果我们移除了自动添加空行，useEffect 将只显示 valid rows。
    // 用户点击 Add -> 本地有了新行 -> config 没变 -> useEffect 没触发 -> 新行保留。
    // 用户填完 -> config 变了 -> useEffect 触发 -> 重置为 config 内容（包含新行）-> 新行状态变非 isNew。正确。
    // 唯一问题：如果用户填了一半，config 没变，但 modelMap 引用变了？
    // 通常 parent 会 ensure stable object if deep equal, or we rely on React rendering.
    setSimpleRows(existingSimple);
  }, [config.modelMap]);

  // 从 config 初始化 Effort 行数据
  useEffect(() => {
    const existingEffort: EffortMappingRow[] = Object.entries(config.effortMap).map(
      ([key, target]) => {
        const [model, effort] = key.split("@");
        return {
          id: `effort-${key}`,
          model,
          effort: effort || "medium",
          target,
          isNew: false,
        };
      }
    );
    setEffortRows(existingEffort);
  }, [config.effortMap]);

  // 同步 enabled 状态
  useEffect(() => {
    setIsOpen(config.enabled);
  }, [config.enabled]);

  // 更新简单映射行
  const handleSimpleRowChange = (
    id: string,
    field: "source" | "target",
    value: string
  ) => {
    setSimpleRows((prev) => {
      const updated = prev.map((row) =>
        row.id === id ? { ...row, [field]: value } : row
      );
      // 同步到 config
      syncSimpleToConfig(updated);
      return updated;
    });
  };

  // 同步简单映射到 config
  const syncSimpleToConfig = (rows: SimpleMappingRow[]) => {
    const newModelMap: Record<string, string> = {};
    rows.forEach((row) => {
      if (row.source.trim() && row.target.trim()) {
        newModelMap[row.source.trim()] = row.target.trim();
      }
    });
    if (JSON.stringify(newModelMap) !== JSON.stringify(config.modelMap)) {
      onConfigChange({ ...config, modelMap: newModelMap });
    }
  };

  // 添加简单映射新行
  const handleAddSimpleRow = () => {
    const newId = `simple-new-${Date.now()}`;
    const newRow: SimpleMappingRow = {
      id: newId,
      source: "",
      target: "",
      isNew: true,
    };
    setSimpleRows((prev) => [...prev, newRow]);
    // Auto-focus
    setTimeout(() => {
      simpleInputRefs.current.get(newId)?.focus();
    }, 50);
  };

  // 删除简单映射行
  const handleRemoveSimpleRow = (id: string) => {
    setSimpleRows((prev) => {
      const updated = prev.filter((row) => row.id !== id);
      syncSimpleToConfig(updated);
      return updated;
    });
  };

  // 更新 effort 映射行
  const handleEffortRowChange = (
    id: string,
    field: "model" | "effort" | "target",
    value: string
  ) => {
    setEffortRows((prev) => {
      const updated = prev.map((row) =>
        row.id === id ? { ...row, [field]: value } : row
      );
      syncEffortToConfig(updated);
      return updated;
    });
  };

  // 同步 effort 映射到 config
  const syncEffortToConfig = (rows: EffortMappingRow[]) => {
    const newEffortMap: Record<string, string> = {};
    rows.forEach((row) => {
      if (row.model.trim() && row.target.trim()) {
        const key = `${row.model.trim()}@${row.effort}`;
        newEffortMap[key] = row.target.trim();
      }
    });
    if (JSON.stringify(newEffortMap) !== JSON.stringify(config.effortMap)) {
      onConfigChange({ ...config, effortMap: newEffortMap });
    }
  };

  // 添加 effort 映射新行
  const handleAddEffortRow = () => {
    const newId = `effort-new-${Date.now()}`;
    const newRow: EffortMappingRow = {
      id: newId,
      model: "",
      effort: "medium",
      target: "",
      isNew: true,
    };
    setEffortRows((prev) => [...prev, newRow]);
    setTimeout(() => {
      effortInputRefs.current.get(newId)?.focus();
    }, 50);
  };

  // 删除 effort 映射行
  const handleRemoveEffortRow = (id: string) => {
    setEffortRows((prev) => {
      const updated = prev.filter((row) => row.id !== id);
      syncEffortToConfig(updated);
      return updated;
    });
  };

  return (
    <div className="rounded-lg border border-border/50 bg-muted/20">
      <button
        type="button"
        className="flex w-full items-center justify-between p-4 hover:bg-muted/30 transition-colors"
        onClick={() => setIsOpen(!isOpen)}
      >
        <div className="flex items-center gap-3">
          <ArrowRightLeft className="h-4 w-4 text-muted-foreground" />
          <span className="font-medium">
            {t("providerAdvanced.codexModelMapping", {
              defaultValue: "Codex 模型映射",
            })}
          </span>
        </div>
        <div className="flex items-center gap-3">
          <div
            className="flex items-center gap-2"
            onClick={(e) => e.stopPropagation()}
          >
            <Label
              htmlFor="codex-mapping-enabled"
              className="text-sm text-muted-foreground"
            >
              {t("providerAdvanced.enableMapping", {
                defaultValue: "启用映射",
              })}
            </Label>
            <Switch
              id="codex-mapping-enabled"
              checked={config.enabled}
              onCheckedChange={(checked) => {
                onConfigChange({ ...config, enabled: checked });
                if (checked) setIsOpen(true);
              }}
            />
          </div>
          {isOpen ? (
            <ChevronDown className="h-4 w-4 text-muted-foreground" />
          ) : (
            <ChevronRight className="h-4 w-4 text-muted-foreground" />
          )}
        </div>
      </button>

      <div
        className={cn(
          "overflow-hidden transition-all duration-300 ease-in-out",
          isOpen ? "max-h-[1000px] opacity-100" : "max-h-0 opacity-0"
        )}
      >
        <div className="border-t border-border/50 p-4 space-y-6">
          <p className="text-sm text-muted-foreground">
            {t("providerAdvanced.codexModelMappingDesc", {
              defaultValue:
                "为 Codex 请求配置模型ID映射，支持简单映射和 effort 组合映射。",
            })}
          </p>

          {/* 简单模型映射 */}
          <div className="space-y-3">
            <Label className="text-sm font-medium">
              {t("providerAdvanced.simpleModelMapping", {
                defaultValue: "简单模型映射",
              })}
            </Label>
            <p className="text-xs text-muted-foreground">
              {t("providerAdvanced.simpleModelMappingHint", {
                defaultValue: "将请求中的模型ID替换为目标模型ID（优先级最高）",
              })}
            </p>
            <div className="flex justify-end">
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={handleAddSimpleRow}
                disabled={!config.enabled}
                className="h-8"
              >
                <Plus className="mr-2 h-4 w-4" />
                {t("common.add", { defaultValue: "添加" })}
              </Button>
            </div>

            {/* 映射行列表 */}
            <div className="space-y-2">
              {simpleRows.map((row) => (
                <div
                  key={row.id}
                  className="flex items-center gap-2"
                >
                  <Input
                    ref={(el) => {
                      if (el) simpleInputRefs.current.set(row.id, el);
                    }}
                    placeholder={t("providerAdvanced.sourceModel", {
                      defaultValue: "请求模型",
                    })}
                    value={row.source}
                    onChange={(e) =>
                      handleSimpleRowChange(row.id, "source", e.target.value)
                    }
                    className="flex-1 font-mono text-sm transition-colors duration-200"
                    disabled={!config.enabled}
                  />
                  <span className="text-muted-foreground">→</span>
                  <Input
                    placeholder={t("providerAdvanced.targetModel", {
                      defaultValue: "目标模型",
                    })}
                    value={row.target}
                    onChange={(e) =>
                      handleSimpleRowChange(row.id, "target", e.target.value)
                    }
                    className="flex-1 font-mono text-sm transition-colors duration-200"
                    disabled={!config.enabled}
                  />
                  {/* 删除按钮 - 总是显示，除非只有一个空行（可选，但这里我们允许删除所有行，或者保持至少一行空的逻辑在 handleRemove 处理） */}
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    onClick={() => handleRemoveSimpleRow(row.id)}
                    disabled={!config.enabled}
                    className="transition-opacity duration-200 hover:bg-destructive/10"
                  >
                    <Trash2 className="h-4 w-4 text-destructive" />
                  </Button>
                </div>
              ))}
            </div>
          </div>

          {/* Effort 组合映射 */}
          <div className="space-y-3">
            <Label className="text-sm font-medium">
              {t("providerAdvanced.effortModelMapping", {
                defaultValue: "Effort 组合映射",
              })}
            </Label>
            <p className="text-xs text-muted-foreground">
              {t("providerAdvanced.effortModelMappingHint", {
                defaultValue:
                  "当请求同时匹配模型ID和 reasoning.effort 时，替换为目标模型ID（仅当简单映射未匹配时生效）",
              })}
            </p>
            <div className="flex justify-end">
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={handleAddEffortRow}
                disabled={!config.enabled}
                className="h-8"
              >
                <Plus className="mr-2 h-4 w-4" />
                {t("common.add", { defaultValue: "添加" })}
              </Button>
            </div>

            {/* 映射行列表 */}
            <div className="space-y-2">
              {effortRows.map((row) => (
                <div
                  key={row.id}
                  className="flex items-center gap-2"
                >
                  <Input
                    ref={(el) => {
                      if (el) effortInputRefs.current.set(row.id, el);
                    }}
                    placeholder={t("providerAdvanced.sourceModel", {
                      defaultValue: "请求模型",
                    })}
                    value={row.model}
                    onChange={(e) =>
                      handleEffortRowChange(row.id, "model", e.target.value)
                    }
                    className="flex-1 font-mono text-sm transition-colors duration-200"
                    disabled={!config.enabled}
                  />
                  <span className="text-muted-foreground">@</span>
                  <Select
                    value={row.effort}
                    onValueChange={(value) =>
                      handleEffortRowChange(row.id, "effort", value)
                    }
                    disabled={!config.enabled}
                  >
                    <SelectTrigger className="w-24 transition-colors duration-200">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {EFFORT_OPTIONS.map((opt) => (
                        <SelectItem key={opt} value={opt}>
                          {opt}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <span className="text-muted-foreground">→</span>
                  <Input
                    placeholder={t("providerAdvanced.targetModel", {
                      defaultValue: "目标模型",
                    })}
                    value={row.target}
                    onChange={(e) =>
                      handleEffortRowChange(row.id, "target", e.target.value)
                    }
                    className="flex-1 font-mono text-sm transition-colors duration-200"
                    disabled={!config.enabled}
                  />
                  {/* 删除按钮 */}
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    onClick={() => handleRemoveEffortRow(row.id)}
                    disabled={!config.enabled}
                    className="transition-opacity duration-200 hover:bg-destructive/10"
                  >
                    <Trash2 className="h-4 w-4 text-destructive" />
                  </Button>
                </div>
              ))}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
