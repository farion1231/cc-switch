import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import {
  AlertCircle,
  CheckCircle2,
  Clipboard,
  Copy,
  FileText,
  KeyRound,
  Loader2,
  Play,
  PlugZap,
  RadioTower,
  RefreshCw,
  Server,
  Settings2,
  ShieldCheck,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { proxyApi } from "@/lib/api/proxy";
import { copyText } from "@/lib/clipboard";
import { useProxyStatus } from "@/hooks/useProxyStatus";
import { cn } from "@/lib/utils";
import {
  buildBaseUrl,
  buildReachableBaseUrl,
  buildJsonConfig,
  buildProfileUpdate,
  buildPythonSnippet,
  chooseDefaultBackendKey,
  describeBackendTarget,
  groupBackendOptions,
  profileBackendKey,
} from "@/lib/openai/externalProfile";
import {
  ExternalBackendPicker,
  SelectedBackendSummary,
} from "@/components/openai/ExternalBackendPicker";

const BACKEND_STORAGE_KEY = "cc-switch-openai-compatible-backend";
const FALLBACK_MODEL = "gpt-5.4-mini";

type ApiTab = "source" | "access" | "config" | "check";

/// 读取用户上次在页面选择的服务来源 key。
function getSavedBackendKey(): string {
  return localStorage.getItem(BACKEND_STORAGE_KEY) ?? "";
}

/// 第三方 Agent OpenAI-compatible API 顶层工具页。
export function OpenAICompatibleApiPage() {
  const queryClient = useQueryClient();
  const { status, isRunning, startProxyServer } = useProxyStatus();
  const [activeTab, setActiveTab] = useState<ApiTab>("source");
  const [visibleApiKey, setVisibleApiKey] = useState("");
  const [selectedBackendKey, setSelectedBackendKey] =
    useState<string>(getSavedBackendKey);
  const [selectedModel, setSelectedModel] = useState("");
  const [isPreparing, setIsPreparing] = useState(false);
  const [listenAddress, setListenAddress] = useState("127.0.0.1");
  const [listenPort, setListenPort] = useState("15721");
  const [isSavingListener, setIsSavingListener] = useState(false);

  const {
    data: runtimeStatus,
    isLoading,
    refetch: refetchRuntimeStatus,
  } = useQuery({
    queryKey: ["externalOpenAIAPIRuntimeStatus"],
    queryFn: () => proxyApi.getExternalOpenAIAPIRuntimeStatus(),
  });

  const { data: proxyConfig } = useQuery({
    queryKey: ["proxyConfig"],
    queryFn: () => proxyApi.getProxyConfig(),
  });

  const profile = runtimeStatus?.profile;
  const backendOptions = runtimeStatus?.backendOptions ?? [];
  const savedBackendKey = profileBackendKey(profile);
  const selectedKey = chooseDefaultBackendKey(backendOptions, [
    selectedBackendKey,
    savedBackendKey,
    runtimeStatus?.selectedBackend?.key,
  ]);
  const selectedBackend =
    backendOptions.find((option) => option.key === selectedKey) ??
    runtimeStatus?.selectedBackend ??
    null;
  const availableModels = selectedBackend?.models ?? [];
  const defaultModel =
    selectedModel ||
    (profile?.defaultModel && availableModels.includes(profile.defaultModel)
      ? profile.defaultModel
      : "") ||
    runtimeStatus?.effectiveModel ||
    availableModels[0] ||
    profile?.defaultModel ||
    FALLBACK_MODEL;
  const effectiveAddress =
    status?.address || proxyConfig?.listen_address || listenAddress;
  const effectivePort =
    status?.port || proxyConfig?.listen_port || Number(listenPort) || 15721;
  const baseUrl = buildBaseUrl(effectiveAddress, effectivePort);
  const reachableBaseUrl = buildReachableBaseUrl(effectiveAddress, effectivePort);
  const agentBaseUrl = reachableBaseUrl;
  const displayApiKey =
    visibleApiKey ||
    (profile?.hasApiKey ? `${profile.apiKeyPrefix ?? "ccsw_"}...` : "尚未生成");
  const runnableApiKey = visibleApiKey || "<generate-key-to-reveal>";
  const statusIssues = runtimeStatus?.issues ?? [];
  const backendGroups = useMemo(
    () => groupBackendOptions(backendOptions),
    [backendOptions],
  );
  const selectedBackendDescription = describeBackendTarget(selectedBackend);
  const hasDraftChanges =
    selectedBackend?.key !== savedBackendKey ||
    (profile?.defaultModel ?? "") !== defaultModel;
  const canSaveSelected = Boolean(selectedBackend?.available);

  useEffect(() => {
    if (!selectedBackendKey && selectedKey) {
      setSelectedBackendKey(selectedKey);
    }
  }, [selectedBackendKey, selectedKey]);

  useEffect(() => {
    if (!proxyConfig) return;
    setListenAddress(proxyConfig.listen_address || "127.0.0.1");
    setListenPort(String(proxyConfig.listen_port || 15721));
  }, [proxyConfig]);

  /// 复制普通配置文本并给出反馈。
  async function handleCopy(text: string, label: string) {
    await copyText(text);
    toast.success(`${label} 已复制`, { closeButton: true });
  }

  /// 重新生成本地 External API key；明文只保存在当前页面状态。
  async function handleRegenerateKey() {
    const result = await proxyApi.regenerateExternalOpenAIAPIKey();
    setVisibleApiKey(result.apiKey);
    await queryClient.invalidateQueries({
      queryKey: ["externalOpenAIAPIRuntimeStatus"],
    });
    toast.success("外部 API Key 已重新生成", { closeButton: true });
    setActiveTab("config");
  }

  /// 保存本地 API 的监听地址和端口；该设置复用全局 proxy_config，不修改任何 app takeover。
  async function handleSaveListenerConfig() {
    if (!proxyConfig) {
      toast.error("代理配置尚未加载完成");
      return;
    }
    const parsedPort = Number(listenPort);
    if (!Number.isInteger(parsedPort) || parsedPort < 1 || parsedPort > 65535) {
      toast.error("端口必须是 1-65535 之间的整数");
      return;
    }

    setIsSavingListener(true);
    try {
      await proxyApi.updateProxyConfig({
        ...proxyConfig,
        listen_address: listenAddress.trim() || "127.0.0.1",
        listen_port: parsedPort,
      });
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["proxyConfig"] }),
        queryClient.invalidateQueries({ queryKey: ["proxyStatus"] }),
      ]);
      toast.success(
        isRunning ? "监听配置已保存，重启本地 API 后生效" : "监听配置已保存",
        { closeButton: true },
      );
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      toast.error(`保存监听配置失败: ${message}`);
    } finally {
      setIsSavingListener(false);
    }
  }

  /// 复制含 API key 的片段；没有明文 key 时拒绝复制占位配置。
  async function handleCopyRunnableConfig(value: string, label: string) {
    if (!visibleApiKey) {
      toast.error("API Key 明文只显示一次，请先生成 Key");
      setActiveTab("access");
      return;
    }
    await handleCopy(value, label);
  }

  /// 更新页面选中的服务来源，并清空模型临时选择。
  function handleBackendChange(value: string) {
    setSelectedBackendKey(value);
    localStorage.setItem(BACKEND_STORAGE_KEY, value);
    setSelectedModel("");
  }

  /// 保存当前服务来源 profile，但不启动 proxy server。
  async function handleSaveProfile(enabled = profile?.enabled ?? false) {
    if (!selectedBackend) {
      toast.error("请先选择一个服务来源");
      return;
    }
    if (!selectedBackend.available) {
      toast.error(selectedBackend.error ?? "当前服务来源还不能接入");
      return;
    }

    try {
      const update = buildProfileUpdate(selectedBackend, defaultModel, enabled);
      await proxyApi.updateExternalOpenAIAPIProfile(update);
      await queryClient.invalidateQueries({
        queryKey: ["externalOpenAIAPIRuntimeStatus"],
      });
      toast.success("第三方 Agent API 配置已保存", { closeButton: true });
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      toast.error(`保存失败: ${message}`);
    }
  }

  /// 保存 profile、必要时生成 key，并启动本地 proxy server。
  async function handlePrepareService() {
    if (!selectedBackend) {
      toast.error("请先选择一个服务来源");
      return;
    }
    if (!selectedBackend.available) {
      toast.error(selectedBackend.error ?? "当前服务来源还不能接入");
      return;
    }

    setIsPreparing(true);
    try {
      if (!visibleApiKey && !profile?.hasApiKey) {
        const result = await proxyApi.regenerateExternalOpenAIAPIKey();
        setVisibleApiKey(result.apiKey);
      }
      const update = buildProfileUpdate(selectedBackend, defaultModel, true);
      await proxyApi.updateExternalOpenAIAPIProfile(update);
      if (!isRunning) await startProxyServer();
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["proxyStatus"] }),
        queryClient.invalidateQueries({ queryKey: ["externalOpenAIAPIRuntimeStatus"] }),
      ]);
      toast.success("第三方 Agent API 已启用", { closeButton: true });
      setActiveTab("config");
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      toast.error(`准备失败: ${message}`);
    } finally {
      setIsPreparing(false);
    }
  }

  /// 刷新 proxy 状态和 External API runtime 状态。
  async function handleRefresh() {
    await Promise.all([
      refetchRuntimeStatus(),
      queryClient.invalidateQueries({ queryKey: ["proxyStatus"] }),
    ]);
  }

  if (isLoading) {
    return (
      <div className="flex h-full items-center justify-center">
        <Loader2 className="h-7 w-7 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col overflow-hidden px-6 py-4">
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto pr-2">
        <ApiHero
          isRunning={isRunning}
          profileEnabled={profile?.enabled === true}
          ready={runtimeStatus?.ready === true}
          baseUrl={baseUrl}
          selectedLabel={selectedBackend?.label ?? "未选择服务来源"}
          onPrepare={() => void handlePrepareService()}
          onRefresh={() => void handleRefresh()}
          isPreparing={isPreparing}
        />

        <Tabs value={activeTab} onValueChange={(value) => setActiveTab(value as ApiTab)}>
          <div className="sticky top-0 z-10 -mx-1 bg-background/95 px-1 py-2 backdrop-blur">
            <TabsList className="grid w-full grid-cols-4 bg-slate-950/40 p-1">
              <ApiTabTrigger value="source" icon={PlugZap} label="服务来源" />
              <ApiTabTrigger value="access" icon={KeyRound} label="访问凭据" />
              <ApiTabTrigger value="config" icon={FileText} label="Agent 配置" />
              <ApiTabTrigger value="check" icon={ShieldCheck} label="检查" />
            </TabsList>
          </div>

          <TabsContent value="source" className="mt-3">
            <div className="grid gap-4 xl:grid-cols-[1fr_380px]">
              <section className="rounded-lg border border-blue-700/40 bg-blue-950/10 p-4">
                <SectionHeader
                  icon={PlugZap}
                  title="选择对外服务来源"
                  detail="第三方 agent 只看到 OpenAI v1 API；这里选择 CC Switch 内部实际调用哪个模型源。"
                />
                <div className="mt-4">
                  <ExternalBackendPicker
                    groups={backendGroups}
                    selectedKey={selectedBackend?.key ?? ""}
                    onSelect={handleBackendChange}
                  />
                </div>
              </section>

              <aside className="space-y-3">
                <SelectedBackendSummary
                  backend={selectedBackend ?? undefined}
                  description={selectedBackendDescription}
                  hasDraftChanges={hasDraftChanges}
                />
                <ModelPicker
                  availableModels={availableModels}
                  defaultModel={defaultModel}
                  selectedModel={selectedModel}
                  onModelChange={setSelectedModel}
                />
                <Button
                  onClick={() => void handleSaveProfile(profile?.enabled ?? false)}
                  disabled={!canSaveSelected || !hasDraftChanges}
                  className="w-full gap-2 bg-blue-600 hover:bg-blue-500"
                >
                  <Settings2 className="h-4 w-4" />
                  保存来源和模型
                </Button>
              </aside>
            </div>
          </TabsContent>

          <TabsContent value="access" className="mt-3">
            <div className="grid gap-4 xl:grid-cols-[1fr_420px]">
              <section className="rounded-lg border border-emerald-700/40 bg-emerald-950/10 p-4">
                <SectionHeader
                  icon={KeyRound}
                  title="本地访问凭据"
                  detail="这个 Key 只保护本机 API，不是上游 OpenAI 或 ChatGPT 的真实凭据。"
                />
                <div className="mt-4 grid gap-3 md:grid-cols-2">
                  <ConfigValue
                    label="base_url"
                    value={baseUrl}
                    tone="emerald"
                    onCopy={() => handleCopy(baseUrl, "base_url")}
                  />
                  <ConfigValue
                    label="api_key"
                    value={displayApiKey}
                    tone="amber"
                    onCopy={() =>
                      visibleApiKey
                        ? handleCopy(visibleApiKey, "api_key")
                        : toast.error("API Key 明文只显示一次，请先生成 Key")
                    }
                  />
                </div>
                <ListenerSettings
                  listenAddress={listenAddress}
                  listenPort={listenPort}
                  baseUrl={baseUrl}
                  reachableBaseUrl={reachableBaseUrl}
                  isRunning={isRunning}
                  isSaving={isSavingListener}
                  onAddressChange={setListenAddress}
                  onPortChange={setListenPort}
                  onSave={() => void handleSaveListenerConfig()}
                />
                <div className="mt-4 flex flex-wrap gap-2">
                  <Button onClick={handleRegenerateKey} className="gap-2 bg-emerald-600 hover:bg-emerald-500">
                    <KeyRound className="h-4 w-4" />
                    生成新的 ccsw_ Key
                  </Button>
                  <Button
                    variant="outline"
                    onClick={() => void handlePrepareService()}
                    disabled={isPreparing || !canSaveSelected}
                    className="gap-2"
                  >
                    {isPreparing ? (
                      <Loader2 className="h-4 w-4 animate-spin" />
                    ) : (
                      <Play className="h-4 w-4" />
                    )}
                    保存并启动 API
                  </Button>
                </div>
              </section>

              <SecurityPanel />
            </div>
          </TabsContent>

          <TabsContent value="config" className="mt-3">
            <div className="grid gap-4 lg:grid-cols-2">
              <SnippetPanel
                title="Agent JSON 配置"
                value={buildJsonConfig(agentBaseUrl, runnableApiKey, defaultModel)}
                onCopy={() =>
                  handleCopyRunnableConfig(
                    buildJsonConfig(agentBaseUrl, runnableApiKey, defaultModel),
                    "Agent JSON 配置",
                  )
                }
              />
              <SnippetPanel
                title="OpenAI Python SDK 示例"
                value={buildPythonSnippet(agentBaseUrl, runnableApiKey, defaultModel)}
                onCopy={() =>
                  handleCopyRunnableConfig(
                    buildPythonSnippet(agentBaseUrl, runnableApiKey, defaultModel),
                    "Python 示例",
                  )
                }
              />
            </div>
          </TabsContent>

          <TabsContent value="check" className="mt-3">
            <CheckTab
              isRunning={isRunning}
              profileEnabled={profile?.enabled === true}
              hasApiKey={profile?.hasApiKey === true || Boolean(visibleApiKey)}
              hasBackend={Boolean(selectedBackend)}
              backendAvailable={selectedBackend?.available === true}
              model={defaultModel}
              issues={statusIssues}
              onRefresh={() => void handleRefresh()}
            />
          </TabsContent>
        </Tabs>
      </div>
    </div>
  );
}

/// 顶部状态区，使用多色状态块说明当前 API 是否可以被第三方 agent 接入。
function ApiHero({
  isRunning,
  profileEnabled,
  ready,
  baseUrl,
  selectedLabel,
  onPrepare,
  onRefresh,
  isPreparing,
}: {
  isRunning: boolean;
  profileEnabled: boolean;
  ready: boolean;
  baseUrl: string;
  selectedLabel: string;
  onPrepare: () => void;
  onRefresh: () => void;
  isPreparing: boolean;
}) {
  return (
    <div className="overflow-hidden rounded-lg border border-slate-700/80 bg-slate-950/30">
      <div className="grid gap-4 border-b border-slate-700/70 bg-gradient-to-r from-emerald-950/50 via-slate-900 to-blue-950/50 p-5 xl:grid-cols-[1fr_auto]">
        <div>
          <div className="flex items-center gap-2 text-xl font-semibold">
            <RadioTower className="h-5 w-5 text-emerald-300" />
            第三方 Agent API
          </div>
          <p className="mt-2 max-w-4xl text-sm leading-6 text-slate-300">
            对外固定提供 OpenAI v1 compatible API。第三方 agent 只需要填写
            base_url、api_key、model；OAuth、真实上游凭据和路由细节都留在 CC Switch 内部。
          </p>
        </div>
        <div className="flex flex-wrap items-start justify-end gap-2">
          <Button onClick={onPrepare} disabled={isPreparing} className="gap-2 bg-emerald-600 hover:bg-emerald-500">
            {isPreparing ? <Loader2 className="h-4 w-4 animate-spin" /> : <Play className="h-4 w-4" />}
            保存并启动
          </Button>
          <Button variant="outline" onClick={onRefresh} className="gap-2">
            <RefreshCw className="h-4 w-4" />
            刷新
          </Button>
        </div>
      </div>
      <div className="grid gap-3 p-4 md:grid-cols-4">
        <HeroMetric
          color="emerald"
          icon={Server}
          label="API 端点"
          value={isRunning ? "运行中" : "未启动"}
          detail={baseUrl}
        />
        <HeroMetric
          color="blue"
          icon={PlugZap}
          label="服务来源"
          value={selectedLabel}
          detail="可在下方选择"
        />
        <HeroMetric
          color="amber"
          icon={KeyRound}
          label="访问状态"
          value={profileEnabled ? "已启用" : "未启用"}
          detail="使用 ccsw_ 本地 Key"
        />
        <HeroMetric
          color={ready ? "emerald" : "rose"}
          icon={ready ? CheckCircle2 : AlertCircle}
          label="接入检查"
          value={ready ? "可接入" : "待配置"}
          detail="不会切换 Codex 当前模型源"
        />
      </div>
    </div>
  );
}

/// 选项卡触发器封装，统一图标和可点击态。
function ApiTabTrigger({
  value,
  icon: Icon,
  label,
}: {
  value: ApiTab;
  icon: React.ComponentType<{ className?: string }>;
  label: string;
}) {
  return (
    <TabsTrigger value={value} className="min-w-0 gap-2">
      <Icon className="h-4 w-4" />
      <span className="hidden sm:inline">{label}</span>
    </TabsTrigger>
  );
}

/// 模型选择区；当后端没有枚举模型时允许用户手填默认模型。
function ModelPicker({
  availableModels,
  defaultModel,
  selectedModel,
  onModelChange,
}: {
  availableModels: string[];
  defaultModel: string;
  selectedModel: string;
  onModelChange: (value: string) => void;
}) {
  return (
    <div className="rounded-lg border border-amber-700/40 bg-amber-950/10 p-4">
      <div className="mb-2 text-sm font-semibold text-slate-100">默认模型</div>
      {availableModels.length > 0 ? (
        <Select value={defaultModel} onValueChange={onModelChange}>
          <SelectTrigger className="border-amber-700/40 bg-slate-950/60">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {availableModels.map((model) => (
              <SelectItem key={model} value={model}>
                {model}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      ) : (
        <input
          value={selectedModel || defaultModel}
          onChange={(event) => onModelChange(event.target.value)}
          className="h-10 w-full rounded-md border border-amber-700/40 bg-slate-950/60 px-3 text-sm outline-none focus:border-amber-400 focus:ring-2 focus:ring-amber-500/20"
        />
      )}
      <p className="mt-2 text-xs leading-5 text-slate-400">
        第三方 agent 请求里也可以显式传 model；这里是默认值和配置示例使用的模型。
      </p>
    </div>
  );
}

/// 监听配置区：控制第三方 Agent API 绑定到哪个地址和端口。
function ListenerSettings({
  listenAddress,
  listenPort,
  baseUrl,
  reachableBaseUrl,
  isRunning,
  isSaving,
  onAddressChange,
  onPortChange,
  onSave,
}: {
  listenAddress: string;
  listenPort: string;
  baseUrl: string;
  reachableBaseUrl: string;
  isRunning: boolean;
  isSaving: boolean;
  onAddressChange: (value: string) => void;
  onPortChange: (value: string) => void;
  onSave: () => void;
}) {
  const isPublicBind = listenAddress === "0.0.0.0" || listenAddress === "::";

  return (
    <div className="mt-4 rounded-lg border border-cyan-700/40 bg-cyan-950/10 p-4">
      <div className="mb-3 flex flex-wrap items-start justify-between gap-3">
        <div>
          <div className="flex items-center gap-2 text-sm font-semibold text-cyan-100">
            <RadioTower className="h-4 w-4 text-cyan-300" />
            监听地址和端口
          </div>
          <p className="mt-1 text-xs leading-5 text-slate-400">
            默认只允许本机访问；需要给局域网或公网 Agent 使用时，可以改成全网监听或指定网卡地址。
          </p>
        </div>
        <Button
          variant="outline"
          onClick={onSave}
          disabled={isSaving}
          className="gap-2 border-cyan-600/50"
        >
          {isSaving ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <Settings2 className="h-4 w-4" />
          )}
          保存监听
        </Button>
      </div>
      <div className="grid gap-3 md:grid-cols-[220px_140px_1fr]">
        <Select value={listenAddress} onValueChange={onAddressChange}>
          <SelectTrigger className="border-cyan-700/40 bg-slate-950/60">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="127.0.0.1">仅本机 127.0.0.1</SelectItem>
            <SelectItem value="0.0.0.0">所有网卡 0.0.0.0</SelectItem>
            <SelectItem value="localhost">localhost</SelectItem>
          </SelectContent>
        </Select>
        <Input
          value={listenPort}
          onChange={(event) => onPortChange(event.target.value)}
          inputMode="numeric"
          placeholder="15721"
          className="border-cyan-700/40 bg-slate-950/60"
        />
        <Input
          value={listenAddress}
          onChange={(event) => onAddressChange(event.target.value)}
          placeholder="自定义地址，例如 192.168.1.20"
          className="border-cyan-700/40 bg-slate-950/60"
        />
      </div>
      <div className="mt-3 grid gap-2 md:grid-cols-2">
        <code className="rounded-md border border-cyan-700/30 bg-black/20 px-3 py-2 text-xs text-cyan-100">
          本机: {baseUrl}
        </code>
        <code className="rounded-md border border-cyan-700/30 bg-black/20 px-3 py-2 text-xs text-cyan-100">
          外部 Agent: {reachableBaseUrl}
        </code>
      </div>
      {isPublicBind && (
        <div className="mt-3 rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs leading-5 text-amber-100">
          全网监听会让同网段或公网转发进来的客户端访问这个 API。请配合防火墙、强随机 ccsw_ Key 和可信网络使用。
        </div>
      )}
      {isRunning && (
        <div className="mt-2 text-xs text-slate-400">
          服务运行中修改监听地址或端口后，需要停止并重新启动本地 API 才会绑定到新地址。
        </div>
      )}
    </div>
  );
}

/// 安全说明区，强调 Key 和 OAuth 的隔离边界。
function SecurityPanel() {
  return (
    <section className="rounded-lg border border-blue-700/40 bg-blue-950/10 p-4">
      <SectionHeader
        icon={ShieldCheck}
        title="隔离边界"
        detail="这个页面只管理第三方 Agent API profile。"
      />
      <div className="mt-4 space-y-2">
        <BoundaryItem ok text="不暴露 OAuth token、refresh token、真实上游 API Key" />
        <BoundaryItem ok text="不切换 Codex 当前模型源，不打开 takeover" />
        <BoundaryItem ok text="API Key 只保护本地 endpoint，数据库只保存 hash" />
        <BoundaryItem ok text="原生 Claude/Gemini 协议不会静默伪装成 OpenAI API" />
      </div>
    </section>
  );
}

/// 检查页展示缺什么，而不是只给出一串英文 issue。
function CheckTab({
  isRunning,
  profileEnabled,
  hasApiKey,
  hasBackend,
  backendAvailable,
  model,
  issues,
  onRefresh,
}: {
  isRunning: boolean;
  profileEnabled: boolean;
  hasApiKey: boolean;
  hasBackend: boolean;
  backendAvailable: boolean;
  model: string;
  issues: string[];
  onRefresh: () => void;
}) {
  return (
    <section className="rounded-lg border border-slate-700 bg-slate-950/40 p-4">
      <SectionHeader
        icon={ShieldCheck}
        title="接入检查"
        detail="这些状态都通过后，外部 agent 才能稳定使用 /v1/chat/completions。"
        action={
          <Button variant="outline" onClick={onRefresh} className="gap-2">
            <RefreshCw className="h-4 w-4" />
            重新检查
          </Button>
        }
      />
      <div className="mt-4 grid gap-3 md:grid-cols-2">
        <ChecklistItem ok={isRunning} label="本地 API 服务已启动" />
        <ChecklistItem ok={profileEnabled} label="第三方 Agent API profile 已启用" />
        <ChecklistItem ok={hasApiKey} label="已生成 ccsw_ 本地访问 Key" />
        <ChecklistItem ok={hasBackend && backendAvailable} label="已选择可接入服务来源" />
        <ChecklistItem ok={Boolean(model)} label={`默认模型：${model || "未选择"}`} />
        <ChecklistItem ok label="Codex 自身服务不受影响" />
      </div>
      {issues.length > 0 && (
        <div className="mt-4 rounded-lg border border-amber-500/30 bg-amber-500/10 p-3">
          <div className="mb-2 text-sm font-semibold text-amber-100">当前问题</div>
          <div className="flex flex-wrap gap-2">
            {issues.map((issue) => (
              <Badge key={issue} variant="outline">
                {translateIssue(issue)}
              </Badge>
            ))}
          </div>
        </div>
      )}
    </section>
  );
}

/// 顶部彩色指标卡。
function HeroMetric({
  color,
  icon: Icon,
  label,
  value,
  detail,
}: {
  color: "emerald" | "blue" | "amber" | "rose";
  icon: React.ComponentType<{ className?: string }>;
  label: string;
  value: string;
  detail: string;
}) {
  const styles = {
    emerald: "border-emerald-500/40 bg-emerald-500/10 text-emerald-200",
    blue: "border-blue-500/40 bg-blue-500/10 text-blue-200",
    amber: "border-amber-500/40 bg-amber-500/10 text-amber-200",
    rose: "border-rose-500/40 bg-rose-500/10 text-rose-200",
  }[color];

  return (
    <div className={cn("min-w-0 rounded-lg border p-3", styles)}>
      <div className="flex items-center justify-between gap-2">
        <span className="text-xs opacity-80">{label}</span>
        <Icon className="h-4 w-4 opacity-80" />
      </div>
      <div className="mt-2 truncate text-lg font-semibold text-white">{value}</div>
      <div className="mt-1 truncate text-xs opacity-75">{detail}</div>
    </div>
  );
}

/// 通用标题行。
function SectionHeader({
  icon: Icon,
  title,
  detail,
  action,
}: {
  icon: React.ComponentType<{ className?: string }>;
  title: string;
  detail: string;
  action?: React.ReactNode;
}) {
  return (
    <div className="flex flex-wrap items-start justify-between gap-3">
      <div className="min-w-0">
        <div className="flex items-center gap-2 text-base font-semibold text-slate-100">
          <Icon className="h-4 w-4 text-blue-300" />
          {title}
        </div>
        <p className="mt-1 text-xs leading-5 text-slate-400">{detail}</p>
      </div>
      {action}
    </div>
  );
}

/// 渲染可复制的配置字段。
function ConfigValue({
  label,
  value,
  tone,
  onCopy,
}: {
  label: string;
  value: string;
  tone: "emerald" | "amber";
  onCopy: () => void;
}) {
  const toneClass =
    tone === "emerald"
      ? "border-emerald-700/40 bg-emerald-950/20"
      : "border-amber-700/40 bg-amber-950/20";
  return (
    <div className={cn("min-w-0 rounded-lg border p-3", toneClass)}>
      <div className="mb-2 text-xs font-medium uppercase text-slate-400">
        {label}
      </div>
      <div className="flex items-center gap-2">
        <code className="min-w-0 flex-1 truncate text-sm text-slate-100">{value}</code>
        <Button variant="ghost" size="icon" onClick={onCopy} title={`复制 ${label}`}>
          <Copy className="h-4 w-4" />
        </Button>
      </div>
    </div>
  );
}

/// 渲染可复制的代码片段。
function SnippetPanel({
  title,
  value,
  onCopy,
}: {
  title: string;
  value: string;
  onCopy: () => void;
}) {
  return (
    <div className="rounded-lg border border-slate-700 bg-slate-950/40 p-4">
      <div className="mb-3 flex items-center justify-between gap-3">
        <div className="flex items-center gap-2 text-sm font-semibold text-slate-100">
          <Clipboard className="h-4 w-4 text-blue-300" />
          {title}
        </div>
        <Button variant="ghost" size="icon" onClick={onCopy} title={`复制 ${title}`}>
          <Copy className="h-4 w-4" />
        </Button>
      </div>
      <pre className="max-h-96 overflow-auto rounded-lg bg-black/30 p-4 text-xs leading-relaxed text-slate-100">
        <code>{value}</code>
      </pre>
    </div>
  );
}

function BoundaryItem({ ok, text }: { ok: boolean; text: string }) {
  return (
    <div className="flex items-center gap-2 rounded-md border border-blue-700/30 bg-slate-950/40 p-2 text-sm text-slate-200">
      {ok ? (
        <CheckCircle2 className="h-4 w-4 text-emerald-300" />
      ) : (
        <AlertCircle className="h-4 w-4 text-amber-300" />
      )}
      {text}
    </div>
  );
}

function ChecklistItem({ ok, label }: { ok: boolean; label: string }) {
  return (
    <div
      className={cn(
        "flex items-center gap-2 rounded-md border p-3 text-sm",
        ok
          ? "border-emerald-500/40 bg-emerald-500/10 text-emerald-100"
          : "border-amber-500/40 bg-amber-500/10 text-amber-100",
      )}
    >
      {ok ? <CheckCircle2 className="h-4 w-4" /> : <AlertCircle className="h-4 w-4" />}
      {label}
    </div>
  );
}

function translateIssue(issue: string): string {
  const translations: Record<string, string> = {
    "profile disabled": "profile 未启用",
    "api key not generated": "还没有生成 ccsw_ Key",
    "backend not selected": "还没有选择服务来源",
    "model not selected": "还没有选择默认模型",
  };
  return translations[issue] ?? issue;
}
