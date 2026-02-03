import { useTranslation } from "react-i18next";
import { useState, useRef } from "react";
import { ChevronDown, ChevronRight, Filter, Plus, Trash2 } from "lucide-react";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Switch } from "@/components/ui/switch";
import { Button } from "@/components/ui/button";

/** 请求体重写器配置 */
export interface RequestBodyRewriterConfig {
    enabled: boolean;
    /** 重写规则：key 为字段路径，value 为新值（null = 删除） */
    rules: Record<string, any>;
}

/** 默认配置 */
export const defaultRequestBodyRewriterConfig: RequestBodyRewriterConfig = {
    enabled: false,
    rules: {},
};

interface RequestBodyRewriterConfigProps {
    config: RequestBodyRewriterConfig;
    onConfigChange: (config: RequestBodyRewriterConfig) => void;
}

/** 规则行类型 */
interface RuleRow {
    id: string;
    path: string;
    value: string; // JSON 字符串，"null" 表示删除
}

/** 预设规则 */
const PRESET_RULES = [
    { label: "text (删除)", path: "text", value: "null" },
    { label: "instructions (删除)", path: "instructions", value: "null" },
    { label: "text.verbosity (删除)", path: "text.verbosity", value: "null" },
];

let rowIdCounter = 0;

export function RequestBodyRewriterConfig({
    config,
    onConfigChange,
}: RequestBodyRewriterConfigProps) {
    const { t } = useTranslation();
    const [isOpen, setIsOpen] = useState(config.enabled);
    const inputRefs = useRef<Map<string, HTMLInputElement>>(new Map());

    // 从 config 初始化行数据（仅在首次渲染时，无默认空行）
    const [rows, setRows] = useState<RuleRow[]>(() => {
        return Object.entries(config.rules || {}).map(
            ([path, value], index) => ({
                id: `initial-${index}`,
                path,
                value: value === null ? "null" : JSON.stringify(value),
            })
        );
    });

    // 同步到 config
    const syncToConfig = (newRows: RuleRow[]) => {
        const rules: Record<string, any> = {};
        for (const row of newRows) {
            if (row.path.trim()) {
                try {
                    rules[row.path.trim()] = JSON.parse(row.value);
                } catch {
                    // 无效 JSON，视为字符串
                    rules[row.path.trim()] = row.value === "null" ? null : row.value;
                }
            }
        }
        onConfigChange({ ...config, rules });
    };

    // 更新行
    const handleRowChange = (
        id: string,
        field: "path" | "value",
        value: string
    ) => {
        setRows((prev) => {
            const newRows = prev.map((row) =>
                row.id === id ? { ...row, [field]: value } : row
            );
            // 延迟同步
            setTimeout(() => syncToConfig(newRows), 0);
            return newRows;
        });
    };

    // 删除行
    const handleRemoveRow = (id: string) => {
        setRows((prev) => {
            const newRows = prev.filter((row) => row.id !== id);
            syncToConfig(newRows);
            return newRows;
        });
    };

    // 添加空行
    const handleAddRow = () => {
        const newRow: RuleRow = {
            id: `row-${++rowIdCounter}`,
            path: "",
            value: "null",
        };
        setRows((prev) => [...prev, newRow]);
        // 延迟聚焦到新行
        setTimeout(() => {
            const input = inputRefs.current.get(`${newRow.id}-path`);
            input?.focus();
        }, 50);
    };

    // 添加预设规则
    const handleAddPreset = (preset: (typeof PRESET_RULES)[0]) => {
        // 检查是否已存在
        if (rows.some((r) => r.path === preset.path)) {
            return;
        }
        const newRow: RuleRow = {
            id: `row-${++rowIdCounter}`,
            path: preset.path,
            value: preset.value,
        };
        setRows((prev) => {
            const newRows = [...prev, newRow];
            syncToConfig(newRows);
            return newRows;
        });
    };

    return (
        <div className="space-y-3">
            {/* 标题栏 */}
            <div
                className="flex items-center gap-2 cursor-pointer select-none"
                onClick={() => setIsOpen(!isOpen)}
            >
                {isOpen ? (
                    <ChevronDown className="h-4 w-4 text-muted-foreground" />
                ) : (
                    <ChevronRight className="h-4 w-4 text-muted-foreground" />
                )}
                <Filter className="h-4 w-4 text-muted-foreground" />
                <span className="text-sm font-medium">
                    {t("provider.requestBodyRewriter", "请求体字段重写")}
                </span>
                <div className="ml-auto" onClick={(e) => e.stopPropagation()}>
                    <Switch
                        checked={config.enabled}
                        onCheckedChange={(checked) =>
                            onConfigChange({ ...config, enabled: checked })
                        }
                    />
                </div>
            </div>

            {/* 配置内容 */}
            {isOpen && config.enabled && (
                <div className="pl-6 space-y-3">
                    {/* 预设按钮和添加按钮 */}
                    <div className="flex flex-wrap items-center gap-2">
                        <span className="text-xs text-muted-foreground">
                            {t("provider.presets", "预设")}:
                        </span>
                        {PRESET_RULES.map((preset) => (
                            <Button
                                key={preset.path}
                                type="button"
                                variant="outline"
                                size="sm"
                                className="h-6 text-xs"
                                onClick={() => handleAddPreset(preset)}
                            >
                                <Plus className="h-3 w-3 mr-1" />
                                {preset.label}
                            </Button>
                        ))}
                        <div className="ml-auto">
                            <Button
                                type="button"
                                variant="outline"
                                size="sm"
                                className="h-6 text-xs"
                                onClick={handleAddRow}
                            >
                                <Plus className="h-3 w-3 mr-1" />
                                {t("common.add", "添加")}
                            </Button>
                        </div>
                    </div>

                    {/* 规则列表 */}
                    {rows.length > 0 && (
                        <div className="space-y-2">
                            <div className="grid grid-cols-[1fr_1fr_32px] gap-2 text-xs text-muted-foreground">
                                <span>{t("provider.fieldPath", "字段路径")}</span>
                                <span>{t("provider.newValue", "新值 (null=删除)")}</span>
                                <span></span>
                            </div>

                            {rows.map((row) => (
                                <div
                                    key={row.id}
                                    className="grid grid-cols-[1fr_1fr_32px] gap-2 items-center"
                                >
                                    <Input
                                        ref={(el) => {
                                            if (el) inputRefs.current.set(`${row.id}-path`, el);
                                        }}
                                        placeholder="text.verbosity"
                                        value={row.path}
                                        onChange={(e) =>
                                            handleRowChange(row.id, "path", e.target.value)
                                        }
                                        className="h-8 text-sm"
                                    />
                                    <Textarea
                                        placeholder='null 或 {"key": "value"}'
                                        value={row.value}
                                        onChange={(e) =>
                                            handleRowChange(row.id, "value", e.target.value)
                                        }
                                        className="min-h-[32px] h-8 text-sm font-mono resize-y"
                                        rows={1}
                                    />
                                    <Button
                                        type="button"
                                        variant="ghost"
                                        size="icon"
                                        className="h-8 w-8 text-destructive hover:text-destructive"
                                        onClick={() => handleRemoveRow(row.id)}
                                    >
                                        <Trash2 className="h-4 w-4" />
                                    </Button>
                                </div>
                            ))}
                        </div>
                    )}

                    {/* 说明 */}
                    <p className="text-xs text-muted-foreground">
                        {t(
                            "provider.rewriterHelp",
                            "使用点分隔路径（如 text.verbosity）访问嵌套字段。设为 null 删除字段，其他 JSON 值覆盖。"
                        )}
                    </p>
                </div>
            )}
        </div>
    );
}
