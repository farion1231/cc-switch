import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import {
  DEFAULT_CODEX_ENHANCEMENTS,
  type CodexEnhancementSettings,
  type CodexWorkbenchSettings,
} from "@/types/codexWorkbench";

const FEATURE_META: {
  key: keyof CodexEnhancementSettings;
  label: string;
  description: string;
}[] = [
  {
    key: "pluginUnlock",
    label: "插件入口解锁",
    description: "显示被隐藏的插件/市场入口",
  },
  {
    key: "autoExpand",
    label: "自动展开",
    description: "自动展开折叠面板与 reasoning 区块",
  },
  {
    key: "sessionDelete",
    label: "会话删除",
    description: "增强会话删除交互",
  },
  {
    key: "wideConversation",
    label: "宽屏会话",
    description: "会话区域占满可用宽度",
  },
  {
    key: "nativeMenu",
    label: "原生菜单位置",
    description: "修正菜单层级与定位",
  },
  {
    key: "userScriptRuntime",
    label: "用户脚本运行时",
    description: "为用户脚本提供宿主标记",
  },
  {
    key: "markdownExport",
    label: "Markdown 导出",
    description: "启用导出标记（后续完善导出）",
  },
  {
    key: "modelSwitcher",
    label: "模型切换",
    description: "模型切换辅助",
  },
  {
    key: "systemPrompt",
    label: "系统提示词",
    description: "代理侧系统提示词替换（页面标记）",
  },
  {
    key: "reasoningResume",
    label: "推理续接",
    description: "GPT 推理续接标记",
  },
  {
    key: "reasoningToken",
    label: "推理 Token",
    description: "推理 Token 展示标记",
  },
];

export interface EnhancementsTabProps {
  settings: CodexWorkbenchSettings | undefined;
  isLoading?: boolean;
  isSaving?: boolean;
  onChange: (next: CodexWorkbenchSettings) => void;
}

export function EnhancementsTab({
  settings,
  isLoading,
  isSaving,
  onChange,
}: EnhancementsTabProps) {
  const { t } = useTranslation();
  const enhancements = settings?.enhancements ?? DEFAULT_CODEX_ENHANCEMENTS;

  const toggle = (key: keyof CodexEnhancementSettings) => {
    if (!settings) return;
    onChange({
      ...settings,
      enhancements: {
        ...settings.enhancements,
        [key]: !settings.enhancements[key],
      },
    });
  };

  const resetDefaults = () => {
    if (!settings) return;
    onChange({
      ...settings,
      enhancements: { ...DEFAULT_CODEX_ENHANCEMENTS },
    });
  };

  if (isLoading || !settings) {
    return (
      <div className="rounded-lg border p-4 text-sm text-muted-foreground">
        {t("codexWorkbench.loading", { defaultValue: "加载中…" })}
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-2">
        <div>
          <h3 className="text-sm font-medium">
            {t("codexWorkbench.enhancementsTitle", {
              defaultValue: "页面增强",
            })}
          </h3>
          <p className="text-xs text-muted-foreground">
            {t("codexWorkbench.enhancementsHint", {
              defaultValue:
                "开关即时写入设置；已运行增强实例可点「重新注入」应用最新配置。",
            })}
          </p>
        </div>
        <Button
          size="sm"
          variant="outline"
          disabled={isSaving}
          onClick={resetDefaults}
        >
          {t("codexWorkbench.resetDefaults", {
            defaultValue: "恢复推荐默认值",
          })}
        </Button>
      </div>

      <div className="grid gap-2 sm:grid-cols-2">
        {FEATURE_META.map((f) => {
          const on = !!enhancements[f.key];
          return (
            <label
              key={f.key}
              className="flex cursor-pointer items-start gap-3 rounded-lg border p-3 hover:bg-muted/40"
            >
              <input
                type="checkbox"
                className="mt-1"
                checked={on}
                disabled={isSaving}
                onChange={() => toggle(f.key)}
              />
              <span className="min-w-0 flex-1">
                <span className="flex items-center gap-2 text-sm font-medium">
                  {f.label}
                  <span
                    className={
                      on
                        ? "rounded bg-emerald-500/15 px-1.5 py-0.5 text-[10px] text-emerald-700 dark:text-emerald-300"
                        : "rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground"
                    }
                  >
                    {on ? "loaded" : "disabled"}
                  </span>
                </span>
                <span className="block text-xs text-muted-foreground">
                  {f.description}
                </span>
              </span>
            </label>
          );
        })}
      </div>
    </div>
  );
}

export default EnhancementsTab;
