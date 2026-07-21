import { useState, useEffect, useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Badge } from "@/components/ui/badge";
import { FullScreenPanel } from "@/components/common/FullScreenPanel";
import { ProviderIcon } from "@/components/ProviderIcon";
import { Loader2, RefreshCw, X, Check } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { cn } from "@/lib/utils";
import { providerPresets as claudePresets } from "@/config/claudeProviderPresets";
import { codexProviderPresets } from "@/config/codexProviderPresets";
import { geminiProviderPresets } from "@/config/geminiProviderPresets";
import type { UpstreamRoute } from "@/types";

interface RouteEntryFormProps {
  isOpen: boolean;
  onClose: () => void;
  onSave: (route: UpstreamRoute) => void;
  editingRoute?: UpstreamRoute | null;
}

const PROTOCOLS = [
  { value: "anthropic", label: "Anthropic (Messages API)" },
  { value: "openai_chat", label: "OpenAI Chat Completions" },
  { value: "openai_responses", label: "OpenAI Responses API" },
  { value: "gemini", label: "Gemini (Google)" },
] as const;

/** 路由模板 */
interface RoutePreset {
  name: string;
  protocol: string;
  baseUrl: string;
  icon?: string;
  iconColor?: string;
}

/** 按协议分组的所有预设 */
function buildPresetsByProtocol(): Record<string, RoutePreset[]> {
  const map: Record<string, RoutePreset[]> = {
    anthropic: [],
    openai_chat: [],
    openai_responses: [],
    gemini: [],
  };
  const seen = new Set<string>();

  function push(p: RoutePreset) {
    const k = `${p.name}::${p.protocol}`;
    if (seen.has(k)) return;
    seen.add(k);
    const list = map[p.protocol];
    if (list) list.push(p);
  }

  // Claude 预设
  for (const p of claudePresets) {
    const url = (p.settingsConfig as any)?.env?.ANTHROPIC_BASE_URL;
    if (!url) continue;
    const protocol =
      p.apiFormat === "gemini_native"
        ? "gemini"
        : p.apiFormat === "openai_chat"
          ? "openai_chat"
          : p.apiFormat === "openai_responses"
            ? "openai_responses"
            : "anthropic";
    push({
      name: p.name,
      protocol,
      baseUrl: url,
      icon: p.icon,
      iconColor: p.iconColor,
    });
  }

  // Codex 预设（只取有 apiFormat 的）
  for (const p of codexProviderPresets) {
    if (!p.apiFormat) continue;
    const url = p.endpointCandidates?.[0] || "";
    if (!url) continue;
    const protocol =
      p.apiFormat === "openai_chat"
        ? "openai_chat"
        : p.apiFormat === "openai_responses"
          ? "openai_responses"
          : "anthropic";
    push({
      name: p.name,
      protocol,
      baseUrl: url,
      icon: p.icon,
      iconColor: p.iconColor,
    });
  }

  // Gemini 预设
  for (const p of geminiProviderPresets) {
    const url = (p as any).baseURL || p.endpointCandidates?.[0] || "";
    if (!url) continue;
    push({
      name: p.name,
      protocol: "gemini",
      baseUrl: url,
      icon: p.icon,
      iconColor: p.iconColor,
    });
  }

  return map;
}

export function RouteEntryForm({
  isOpen,
  onClose,
  onSave,
  editingRoute,
}: RouteEntryFormProps) {
  const { t } = useTranslation();
  const isEdit = !!editingRoute;

  const presetsByProtocol = useMemo(() => buildPresetsByProtocol(), []);
  const [protocol, setProtocol] = useState("openai_chat");
  const [name, setName] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [enabled, setEnabled] = useState(true);
  const [modelNames, setModelNames] = useState<string[]>([]);
  const [fetching, setFetching] = useState(false);
  const [customModel, setCustomModel] = useState("");
  const [isCustom, setIsCustom] = useState(true);
  const [presetSearch, setPresetSearch] = useState("");

  useEffect(() => {
    if (editingRoute) {
      setName(editingRoute.name);
      setProtocol(editingRoute.protocol || "openai_chat");
      setBaseUrl(editingRoute.baseUrl);
      setApiKey(editingRoute.apiKey);
      setEnabled(editingRoute.enabled);
      setModelNames(editingRoute.modelNames || []);
      setIsCustom(true);
    } else {
      resetForm();
    }
    setCustomModel("");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editingRoute, isOpen]);

  function resetForm() {
    setProtocol("openai_chat");
    setName("");
    setBaseUrl("");
    setApiKey("");
    setEnabled(true);
    setModelNames([]);
    setIsCustom(true);
  }

  function applyPreset(preset: RoutePreset) {
    setName(preset.name);
    setProtocol(preset.protocol);
    setBaseUrl(preset.baseUrl);
    setIsCustom(false);
  }

  function useCustom() {
    setIsCustom(true);
  }

  const handleFetchModels = useCallback(async () => {
    if (!baseUrl.trim() || !apiKey.trim()) {
      toast.error("请先填写上游地址和 API Key");
      return;
    }
    setFetching(true);
    try {
      const models: string[] = await invoke("fetch_upstream_models", {
        protocol,
        baseUrl: baseUrl.trim(),
        apiKey: apiKey.trim(),
      });
      setModelNames(models);
      toast.success(`获取到 ${models.length} 个模型`);
    } catch (err) {
      toast.error(`获取模型列表失败: ${err}`);
    } finally {
      setFetching(false);
    }
  }, [protocol, baseUrl, apiKey]);

  const addCustomModel = () => {
    const m = customModel.trim();
    if (m && !modelNames.includes(m)) {
      setModelNames((prev) => [...prev, m]);
      setCustomModel("");
    }
  };

  const removeModel = (model: string) => {
    setModelNames((prev) => prev.filter((m) => m !== model));
  };

  const handleSubmit = () => {
    if (!name.trim() || !baseUrl.trim() || !apiKey.trim()) return;
    onSave({
      id: editingRoute?.id || crypto.randomUUID(),
      name: name.trim(),
      protocol,
      baseUrl: baseUrl.trim(),
      apiKey: apiKey.trim(),
      modelNames,
      enabled,
    });
    onClose();
  };

  const canSave =
    name.trim().length > 0 &&
    baseUrl.trim().length > 0 &&
    apiKey.trim().length > 0;

  const footer = (
    <>
      <Button variant="outline" onClick={onClose}>
        {t("common.cancel", { defaultValue: "取消" })}
      </Button>
      <Button onClick={handleSubmit} disabled={!canSave}>
        {t("common.save", { defaultValue: "保存" })}
      </Button>
    </>
  );

  const currentPresets = presetsByProtocol[protocol] || [];

  return (
    <FullScreenPanel
      isOpen={isOpen}
      title={isEdit ? "编辑路由目标" : "添加路由目标"}
      onClose={onClose}
      footer={footer}
    >
      <div className="space-y-6">
        {/* 编辑模式：显示当前信息 + 协议可选 */}
        {isEdit ? (
          <div className="space-y-3">
            <div className="flex items-center gap-2 rounded-lg border bg-accent/50 p-3 text-sm">
              <ProviderIcon name={name} size={18} />
              <span>{name}</span>
              <Badge variant="secondary" className="ml-auto text-[10px]">
                {protocol}
              </Badge>
            </div>
            <div className="space-y-2">
              <Label htmlFor="edit-protocol">协议类型</Label>
              <Select value={protocol} onValueChange={setProtocol}>
                <SelectTrigger id="edit-protocol">
                  <SelectValue placeholder="选择协议类型" />
                </SelectTrigger>
                <SelectContent>
                  {PROTOCOLS.map((p) => (
                    <SelectItem key={p.value} value={p.value}>
                      {p.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
        ) : (
          <>
            {/* 第一步：选协议 */}
            <div className="space-y-2">
              <Label>选择协议类型</Label>
              <Select
                value={protocol}
                onValueChange={(v) => {
                  setProtocol(v);
                  setIsCustom(true);
                  setName("");
                  setBaseUrl("");
                }}
              >
                <SelectTrigger>
                  <SelectValue placeholder="选择协议类型" />
                </SelectTrigger>
                <SelectContent>
                  {PROTOCOLS.map((p) => (
                    <SelectItem key={p.value} value={p.value}>
                      {p.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {/* 第二步：选预设 */}
            {currentPresets.length > 0 && (
              <div className="space-y-2">
                <Label>选择供应商预设（可选）</Label>
                <Input
                  value={presetSearch}
                  onChange={(e) => setPresetSearch(e.target.value)}
                  placeholder="搜索供应商..."
                  className="h-8 text-xs"
                />
                <div className="grid max-h-48 gap-1.5 overflow-y-auto rounded-lg border p-2">
                  {/* 自定义选项 */}
                  <button
                    onClick={useCustom}
                    className={cn(
                      "flex items-center gap-2 rounded-md px-3 py-2 text-left text-sm transition-colors",
                      isCustom ? "bg-accent font-medium" : "hover:bg-accent/50",
                    )}
                  >
                    <div className="flex h-6 w-6 items-center justify-center rounded bg-muted text-[10px]">
                      +
                    </div>
                    <span>自定义（手动配置）</span>
                    {isCustom && <Check className="ml-auto h-4 w-4" />}
                  </button>

                  <div className="border-t" />

                  {currentPresets
                    .filter((pr) =>
                      pr.name
                        .toLowerCase()
                        .includes(presetSearch.toLowerCase()),
                    )
                    .map((pr) => (
                      <button
                        key={`${pr.name}::${pr.protocol}`}
                        onClick={() => applyPreset(pr)}
                        className={cn(
                          "flex items-center gap-2 rounded-md px-3 py-2 text-left text-sm transition-colors",
                          !isCustom && name === pr.name
                            ? "bg-accent font-medium"
                            : "hover:bg-accent/50",
                        )}
                      >
                        {pr.icon ? (
                          <div className="flex h-6 w-6 items-center justify-center rounded">
                            <ProviderIcon name={pr.icon} size={14} />
                          </div>
                        ) : (
                          <div className="flex h-6 w-6 items-center justify-center rounded bg-muted text-[10px]">
                            ?
                          </div>
                        )}
                        <div className="min-w-0">
                          <p className="truncate">{pr.name}</p>
                          <p className="truncate text-[10px] text-muted-foreground">
                            {pr.baseUrl}
                          </p>
                        </div>
                        {!isCustom && name === pr.name && (
                          <Check className="ml-auto h-4 w-4 shrink-0" />
                        )}
                      </button>
                    ))}
                </div>
              </div>
            )}
          </>
        )}

        {/* 名称 */}
        <div className="space-y-2">
          <Label htmlFor="route-name">名称</Label>
          <Input
            id="route-name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="例如：Anthropic 官方"
          />
        </div>

        {/* 上游地址 */}
        <div className="space-y-2">
          <Label htmlFor="route-baseUrl">上游地址</Label>
          <Input
            id="route-baseUrl"
            value={baseUrl}
            onChange={(e) => setBaseUrl(e.target.value)}
            placeholder="https://api.anthropic.com"
          />
        </div>

        {/* API Key */}
        <div className="space-y-2">
          <Label htmlFor="route-apiKey">API Key</Label>
          <Input
            id="route-apiKey"
            type="password"
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            placeholder="sk-..."
          />
        </div>

        {/* 模型列表 */}
        <div className="space-y-3">
          <div className="flex items-center justify-between">
            <Label>模型列表</Label>
            <Button
              size="sm"
              variant="outline"
              className="h-7 text-xs"
              onClick={handleFetchModels}
              disabled={fetching}
            >
              {fetching ? (
                <Loader2 className="mr-1 h-3.5 w-3.5 animate-spin" />
              ) : (
                <RefreshCw className="mr-1 h-3.5 w-3.5" />
              )}
              从上游获取
            </Button>
          </div>

          {modelNames.length > 0 && (
            <div className="flex flex-wrap gap-1.5 rounded-lg border p-3">
              {modelNames.map((m) => (
                <Badge
                  key={m}
                  variant="secondary"
                  className="cursor-pointer gap-1 pr-1"
                  onClick={() => removeModel(m)}
                >
                  {m}
                  <X className="h-3 w-3" />
                </Badge>
              ))}
            </div>
          )}

          <div className="flex gap-2">
            <Input
              value={customModel}
              onChange={(e) => setCustomModel(e.target.value)}
              placeholder="手动输入模型名（支持 * 通配符）"
              className="h-8 text-xs"
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  addCustomModel();
                }
              }}
            />
            <Button
              size="sm"
              variant="secondary"
              className="h-8 shrink-0 text-xs"
              onClick={addCustomModel}
              disabled={!customModel.trim()}
            >
              添加
            </Button>
          </div>

          <p className="text-xs text-muted-foreground">
            点击模型标签删除，支持通配符如 <code>claude-*</code>
          </p>
        </div>

        {/* 启用开关 */}
        <div className="flex items-center gap-3 rounded-lg border p-3">
          <Switch checked={enabled} onCheckedChange={setEnabled} />
          <div className="space-y-0.5">
            <Label className="text-sm font-medium">启用</Label>
            <p className="text-xs text-muted-foreground">
              启用后参与代理路由匹配
            </p>
          </div>
        </div>
      </div>
    </FullScreenPanel>
  );
}
