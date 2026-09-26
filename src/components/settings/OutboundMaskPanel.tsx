import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Plus, Trash2 } from "lucide-react";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ImeSafeInput } from "@/components/ui/ime-safe-input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  settingsApi,
  type BuiltinMaskRule,
  type MaskCustomRule,
  type MaskRuleKind,
  type MaskOnError,
  type OutboundMaskConfig,
} from "@/lib/api/settings";

const EMPTY_CONFIG: OutboundMaskConfig = {
  enabled: false,
  onError: "warnAndBypass",
  builtin: {},
  customRules: [],
};

export function OutboundMaskPanel() {
  const { t } = useTranslation();
  const [config, setConfig] = useState<OutboundMaskConfig>(EMPTY_CONFIG);
  const [builtinRules, setBuiltinRules] = useState<BuiltinMaskRule[]>([]);
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    Promise.all([
      settingsApi.getOutboundMaskConfig(),
      settingsApi.listBuiltinMaskRules(),
    ])
      .then(([cfg, rules]) => {
        setConfig(cfg);
        setBuiltinRules(rules);
      })
      .catch((e) => console.error("Failed to load outbound mask config:", e))
      .finally(() => setIsLoading(false));
  }, []);

  // 输入框在 blur 时才落盘，那一刻拿到的 config 闭包可能落后一次渲染，
  // 用 ref 始终指向最新一份，避免把用户刚敲进去的字符丢掉。
  const configRef = useRef(config);
  useEffect(() => {
    configRef.current = config;
  }, [config]);

  // 保存失败时回滚到上一份配置，避免界面显示的状态与后端不一致
  const save = async (next: OutboundMaskConfig) => {
    const previous = configRef.current;
    setConfig(next);
    configRef.current = next;
    try {
      await settingsApi.setOutboundMaskConfig(next);
    } catch (e) {
      console.error("Failed to save outbound mask config:", e);
      toast.error(String(e));
      setConfig(previous);
      configRef.current = previous;
    }
  };

  /** 提交输入框的暂存改动 */
  const commitDraft = () => save(configRef.current);

  const isBuiltinOn = (rule: BuiltinMaskRule) =>
    config.builtin[rule.id] ?? rule.defaultOn;

  const setBuiltin = (id: string, on: boolean) =>
    save({ ...config, builtin: { ...config.builtin, [id]: on } });

  const updateCustomRule = (
    index: number,
    updates: Partial<MaskCustomRule>,
  ) => {
    const customRules = config.customRules.map((rule, i) =>
      i === index ? { ...rule, ...updates } : rule,
    );
    return { ...config, customRules };
  };

  const addCustomRule = () =>
    save({
      ...config,
      customRules: [
        ...config.customRules,
        { enabled: true, kind: "literal", label: "", pattern: "" },
      ],
    });

  const removeCustomRule = (index: number) =>
    save({
      ...config,
      customRules: config.customRules.filter((_, i) => i !== index),
    });

  if (isLoading) return null;

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="space-y-0.5">
          <Label>{t("settings.advanced.outboundMask.enabled")}</Label>
          <p className="text-xs text-muted-foreground">
            {t("settings.advanced.outboundMask.enabledDescription")}
          </p>
        </div>
        <Switch
          checked={config.enabled}
          onCheckedChange={(checked) => save({ ...config, enabled: checked })}
        />
      </div>

      <div className="space-y-4">
        <h4 className="text-sm font-medium text-muted-foreground">
          {t("settings.advanced.outboundMask.builtinGroup")}
        </h4>
        {builtinRules.map((rule) => (
          <div
            key={rule.id}
            className="flex items-center justify-between gap-4 pl-4"
          >
            <div className="min-w-0 space-y-0.5">
              <div className="flex items-center gap-2">
                <Label>
                  {t(`settings.advanced.outboundMask.rules.${rule.id}`)}
                </Label>
                {/* 展示占位符形态，让用户一眼看懂模型会收到什么 */}
                <Badge variant="secondary" className="font-mono text-[10px]">
                  {`{{${rule.label}_…}}`}
                </Badge>
              </div>
              <p className="text-xs text-muted-foreground">
                {t(
                  `settings.advanced.outboundMask.rules.${rule.id}Description`,
                )}
              </p>
            </div>
            <Switch
              checked={isBuiltinOn(rule)}
              disabled={!config.enabled}
              onCheckedChange={(checked) => setBuiltin(rule.id, checked)}
            />
          </div>
        ))}
      </div>

      <div className="space-y-4">
        <div className="flex items-center justify-between">
          <h4 className="text-sm font-medium text-muted-foreground">
            {t("settings.advanced.outboundMask.customGroup")}
          </h4>
          <Button
            variant="outline"
            size="sm"
            disabled={!config.enabled}
            onClick={addCustomRule}
          >
            <Plus className="mr-1 h-3.5 w-3.5" />
            {t("settings.advanced.outboundMask.addRule")}
          </Button>
        </div>

        {config.customRules.length === 0 ? (
          <p className="pl-4 text-xs text-muted-foreground">
            {t("settings.advanced.outboundMask.customEmpty")}
          </p>
        ) : (
          <div className="space-y-3 pl-4">
            {config.customRules.map((rule, index) => (
              <div
                key={index}
                className="flex flex-wrap items-center gap-2 rounded-md border p-3"
              >
                <Switch
                  checked={rule.enabled}
                  disabled={!config.enabled}
                  onCheckedChange={(checked) =>
                    save(updateCustomRule(index, { enabled: checked }))
                  }
                />
                <Select
                  value={rule.kind}
                  disabled={!config.enabled}
                  onValueChange={(v) =>
                    save(updateCustomRule(index, { kind: v as MaskRuleKind }))
                  }
                >
                  <SelectTrigger className="h-9 w-28">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="literal">
                      {t("settings.advanced.outboundMask.kindLiteral")}
                    </SelectItem>
                    <SelectItem value="regex">
                      {t("settings.advanced.outboundMask.kindRegex")}
                    </SelectItem>
                  </SelectContent>
                </Select>
                <ImeSafeInput
                  className="h-9 w-28"
                  value={rule.label}
                  disabled={!config.enabled}
                  placeholder={t(
                    "settings.advanced.outboundMask.labelPlaceholder",
                  )}
                  onValueChange={(v) => {
                    const next = updateCustomRule(index, { label: v });
                    setConfig(next);
                    configRef.current = next;
                  }}
                  // 文本落盘放在 blur：逐字保存会把半截正则送去后端校验而弹错
                  onBlur={commitDraft}
                />
                <ImeSafeInput
                  className="h-9 min-w-[12rem] flex-1"
                  value={rule.pattern}
                  disabled={!config.enabled}
                  placeholder={t(
                    "settings.advanced.outboundMask.patternPlaceholder",
                  )}
                  onValueChange={(v) => {
                    const next = updateCustomRule(index, { pattern: v });
                    setConfig(next);
                    configRef.current = next;
                  }}
                  onBlur={commitDraft}
                />
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-9 w-9 shrink-0"
                  disabled={!config.enabled}
                  onClick={() => removeCustomRule(index)}
                  aria-label={t("settings.advanced.outboundMask.removeRule")}
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="flex items-center justify-between gap-4">
        <div className="space-y-0.5">
          <Label>{t("settings.advanced.outboundMask.onError")}</Label>
          <p className="text-xs text-muted-foreground">
            {t("settings.advanced.outboundMask.onErrorDescription")}
          </p>
        </div>
        <Select
          value={config.onError}
          disabled={!config.enabled}
          onValueChange={(v) => save({ ...config, onError: v as MaskOnError })}
        >
          <SelectTrigger className="h-9 w-44 shrink-0">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="warnAndBypass">
              {t("settings.advanced.outboundMask.onErrorBypass")}
            </SelectItem>
            <SelectItem value="blockRequest">
              {t("settings.advanced.outboundMask.onErrorBlock")}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>
    </div>
  );
}
