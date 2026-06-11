import { useMemo, useState } from "react";
import {
  Activity,
  ArrowRight,
  CheckCircle2,
  Clipboard,
  Database,
  FileClock,
  GitBranch,
  Layers3,
  Pencil,
  Play,
  Plus,
  RadioTower,
  Route,
  Server,
  Settings2,
  ShieldCheck,
  Trash2,
  Wand2,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { cn } from "@/lib/utils";
import type { Provider } from "@/types";

type WorkspaceTab = "overview" | "sources" | "routes" | "test" | "records";

type CodexRoute = {
  id?: string;
  label?: string;
  enabled?: boolean;
  match?: {
    models?: string[];
    prefixes?: string[];
  };
  upstream?: {
    baseUrl?: string;
    base_url?: string;
    apiFormat?: string;
    wireApi?: string;
    wire_api?: string;
    auth?: {
      source?: string;
    };
  };
  capabilities?: {
    textOnly?: boolean;
    supportsReasoning?: boolean;
    inputModalities?: string[];
  };
};

type CodexRouting = {
  enabled?: boolean;
  defaultRouteId?: string;
  routes?: CodexRoute[];
};

type RouteRecord = {
  id: string;
  action: string;
  detail: string;
  time: string;
};

type RouteEntry = {
  provider: Provider;
  route: CodexRoute;
  index: number;
};

/// 从 Provider 私有配置里读取 Codex 多模型路由配置；没有配置时返回 null，避免把普通模型源误判成路由方案。
function readCodexRouting(provider: Provider): CodexRouting | null {
  const routing = provider.settingsConfig?.codexRouting;
  if (!routing || typeof routing !== "object") return null;
  return routing as CodexRouting;
}

/// 判断一个 Provider 是否已经承载 Codex 多模型路由；即使暂时关闭，只要有规则也归为路由方案方便继续编辑。
function isRoutingPlan(provider: Provider): boolean {
  const routing = readCodexRouting(provider);
  return Boolean(
    routing &&
      (routing.enabled !== false || (routing.routes?.length ?? 0) > 0),
  );
}

/// 提取 route 的上游协议名，兼容历史字段和 UI 字段。
function routeApiFormat(route: CodexRoute): string {
  return (
    route.upstream?.apiFormat ??
    route.upstream?.wireApi ??
    route.upstream?.wire_api ??
    "openai_chat"
  );
}

/// 提取 route 的上游地址；缺省时说明会继承所在 Provider 的地址。
function routeBaseUrl(route: CodexRoute): string {
  return route.upstream?.baseUrl ?? route.upstream?.base_url ?? "继承模型源地址";
}

/// 把内部认证枚举翻译成页面可理解的中文说明，避免把 provider_config 这类工程词直接丢给用户。
function authSourceLabel(source?: string): string {
  switch (source) {
    case "managed_codex_oauth":
      return "托管 Codex OAuth";
    case "managed_account":
      return "托管账号";
    case "provider_config":
      return "使用路由 API Key";
    default:
      return "继承模型源凭据";
  }
}

/// 把内部协议枚举翻译成用户能识别的接口类型。
function apiFormatLabel(format: string): string {
  switch (format) {
    case "openai_responses":
      return "OpenAI Responses";
    case "openai_messages":
      return "OpenAI Messages";
    case "openai_chat":
      return "OpenAI Chat";
    default:
      return format;
  }
}

/// 汇总 route 可匹配的模型名和前缀，用于列表和测试页展示。
function routeMatchSummary(route: CodexRoute): string {
  const models = route.match?.models?.filter(Boolean) ?? [];
  const prefixes = route.match?.prefixes?.filter(Boolean) ?? [];
  const parts = [
    models.length > 0 ? `精确模型：${models.join(", ")}` : "",
    prefixes.length > 0 ? `模型前缀：${prefixes.join(", ")}` : "",
  ].filter(Boolean);
  return parts.join("；") || "尚未设置匹配条件";
}

/// 收集所有可被 Codex 请求命中的模型名，测试页会优先使用这些真实规则生成候选项。
function collectRouteModels(routes: RouteEntry[]): string[] {
  const modelNames = routes.flatMap(({ route }) => [
    ...(route.match?.models ?? []),
    ...(route.match?.prefixes ?? []).map((prefix) => `${prefix}*`),
  ]);
  return Array.from(new Set(modelNames.filter(Boolean)));
}

/// 显示 Codex 多模型路由工作台；它只复用 Provider 配置，不创建第二套数据库或切换 current provider。
export function CodexRouterWorkspacePage({
  providers,
  onEditProvider,
  onCreateProvider,
}: {
  providers: Provider[];
  onEditProvider: (provider: Provider) => void;
  onCreateProvider: () => void;
}) {
  const [activeTab, setActiveTab] = useState<WorkspaceTab>("overview");
  const [selectedPlanId, setSelectedPlanId] = useState<string | null>(null);
  const [selectedRouteKey, setSelectedRouteKey] = useState<string | null>(null);
  const [testModel, setTestModel] = useState("");
  const [testResult, setTestResult] = useState<string | null>(null);
  const [records, setRecords] = useState<RouteRecord[]>([]);

  const routingPlans = providers.filter(isRoutingPlan);
  const modelSources = providers.filter((provider) => !isRoutingPlan(provider));
  const routeEntries = routingPlans.flatMap((provider) =>
    (readCodexRouting(provider)?.routes ?? []).map((route, index) => ({
      provider,
      route,
      index,
    })),
  );
  const enabledRoutes = routeEntries.filter(({ route }) => route.enabled !== false);
  const routeModels = collectRouteModels(routeEntries);
  const selectedPlan =
    routingPlans.find((provider) => provider.id === selectedPlanId) ??
    routingPlans[0] ??
    null;
  const selectedRouting = selectedPlan ? readCodexRouting(selectedPlan) : null;
  const selectedRoute =
    routeEntries.find(
      ({ provider, route, index }) =>
        `${provider.id}:${route.id ?? index}` === selectedRouteKey,
    ) ?? routeEntries[0];

  const visibleRecords = useMemo(
    () =>
      records.length > 0
        ? records
        : [
            {
              id: "initial-overview",
              action: "读取配置",
              detail: "已从现有 Provider 配置生成路由工作台视图",
              time: "当前会话",
            },
          ],
    [records],
  );

  /// 记录页面内的关键操作，帮助用户知道自己刚刚点过什么；真实持久化仍由 Provider 编辑表单负责。
  function pushRecord(action: string, detail: string) {
    setRecords((current) => [
      {
        id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
        action,
        detail,
        time: new Date().toLocaleTimeString("zh-CN", {
          hour: "2-digit",
          minute: "2-digit",
          second: "2-digit",
        }),
      },
      ...current,
    ]);
  }

  /// 新建路由方案会打开现有 Provider 创建流程，避免出现两套配置来源。
  function handleCreatePlan() {
    pushRecord("创建", "打开创建多路路由表单");
    onCreateProvider();
  }

  /// 编辑路由方案会进入现有 Provider 编辑表单；该表单里可以增删改具体 route。
  function handleEditPlan(provider: Provider, detail = "打开路由方案编辑表单") {
    pushRecord("编辑", `${provider.name}：${detail}`);
    onEditProvider(provider);
  }

  /// 选择方案只改变工作台焦点，不修改数据库。
  function handleSelectPlan(provider: Provider) {
    setSelectedPlanId(provider.id);
    setActiveTab("routes");
    pushRecord("查看", `切换到路由方案：${provider.name}`);
  }

  /// 选择规则后跳转到规则页，让卡片产生明确的可操作反馈。
  function handleSelectRoute(entry: RouteEntry) {
    setSelectedPlanId(entry.provider.id);
    setSelectedRouteKey(`${entry.provider.id}:${entry.route.id ?? entry.index}`);
    setActiveTab("routes");
    pushRecord("查看", `查看规则：${entry.route.label || entry.route.id || "未命名规则"}`);
  }

  /// 页面内测试只做规则匹配预览，不发真实上游请求，避免误触发计费或账号请求。
  function handlePreviewRoute() {
    const model = testModel.trim();
    if (!model) {
      setTestResult("请输入一个 Codex 请求里的 model，例如 gpt-5.4-mini 或 qwen3.6。");
      pushRecord("测试", "未输入模型名，未执行匹配预览");
      return;
    }

    const matched = enabledRoutes.find(({ route }) => {
      const models = route.match?.models ?? [];
      const prefixes = route.match?.prefixes ?? [];
      return models.includes(model) || prefixes.some((prefix) => model.startsWith(prefix));
    });

    if (matched) {
      const result = `${model} 会命中「${matched.route.label || matched.route.id || "未命名规则"}」，上游为 ${routeBaseUrl(matched.route)}。`;
      setTestResult(result);
      pushRecord("测试", result);
      return;
    }

    const fallback = selectedRouting?.defaultRouteId
      ? `没有精确命中，会走默认路由 ${selectedRouting.defaultRouteId}。`
      : "没有命中任何启用规则，且当前方案没有默认路由。";
    setTestResult(fallback);
    pushRecord("测试", `${model}：${fallback}`);
  }

  return (
    <div className="flex h-full flex-col overflow-hidden px-6 py-4">
      <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto pr-2">
        <HeaderPanel
          routingPlans={routingPlans}
          modelSources={modelSources}
          routeEntries={routeEntries}
          enabledRoutes={enabledRoutes}
          onCreatePlan={handleCreatePlan}
          onJump={(tab) => setActiveTab(tab)}
        />

        <Tabs value={activeTab} onValueChange={(value) => setActiveTab(value as WorkspaceTab)}>
          <div className="sticky top-0 z-10 -mx-1 bg-background/95 px-1 py-2 backdrop-blur">
            <TabsList className="grid w-full grid-cols-5 bg-slate-950/40 p-1">
              <WorkspaceTabTrigger value="overview" icon={Layers3} label="总览" />
              <WorkspaceTabTrigger value="sources" icon={Server} label="模型源" />
              <WorkspaceTabTrigger value="routes" icon={Route} label="路由规则" />
              <WorkspaceTabTrigger value="test" icon={Play} label="测试发布" />
              <WorkspaceTabTrigger value="records" icon={FileClock} label="操作记录" />
            </TabsList>
          </div>

          <TabsContent value="overview" className="mt-3">
            <OverviewTab
              routingPlans={routingPlans}
              routeEntries={routeEntries}
              modelSources={modelSources}
              onCreatePlan={handleCreatePlan}
              onSelectPlan={handleSelectPlan}
              onSelectRoute={handleSelectRoute}
              onJump={setActiveTab}
            />
          </TabsContent>

          <TabsContent value="sources" className="mt-3">
            <SourcesTab
              modelSources={modelSources}
              routingPlans={routingPlans}
              onCreatePlan={handleCreatePlan}
              onEditPlan={handleEditPlan}
              onSelectPlan={handleSelectPlan}
            />
          </TabsContent>

          <TabsContent value="routes" className="mt-3">
            <RoutesTab
              routingPlans={routingPlans}
              routeEntries={routeEntries}
              selectedPlan={selectedPlan}
              selectedRoute={selectedRoute}
              onCreatePlan={handleCreatePlan}
              onEditPlan={handleEditPlan}
              onSelectPlan={handleSelectPlan}
              onSelectRoute={handleSelectRoute}
            />
          </TabsContent>

          <TabsContent value="test" className="mt-3">
            <TestTab
              selectedPlan={selectedPlan}
              selectedRouting={selectedRouting}
              routeModels={routeModels}
              testModel={testModel}
              testResult={testResult}
              onModelChange={setTestModel}
              onPreviewRoute={handlePreviewRoute}
              onEditPlan={handleEditPlan}
            />
          </TabsContent>

          <TabsContent value="records" className="mt-3">
            <RecordsTab
              records={visibleRecords}
              onCreatePlan={handleCreatePlan}
              onClear={() => {
                setRecords([]);
                setTestResult(null);
              }}
            />
          </TabsContent>
        </Tabs>
      </div>
    </div>
  );
}

/// 顶部工作台总览，使用更强的色块和按钮态区分“可点击动作”和“只读状态”。
function HeaderPanel({
  routingPlans,
  modelSources,
  routeEntries,
  enabledRoutes,
  onCreatePlan,
  onJump,
}: {
  routingPlans: Provider[];
  modelSources: Provider[];
  routeEntries: RouteEntry[];
  enabledRoutes: RouteEntry[];
  onCreatePlan: () => void;
  onJump: (tab: WorkspaceTab) => void;
}) {
  return (
    <div className="overflow-hidden rounded-lg border border-slate-700/80 bg-slate-950/30">
      <div className="grid gap-4 border-b border-slate-700/70 bg-gradient-to-r from-blue-950/60 via-slate-900 to-emerald-950/40 p-5 xl:grid-cols-[1.3fr_1fr]">
        <div className="space-y-3">
          <div className="flex items-center gap-2 text-xl font-semibold">
            <GitBranch className="h-5 w-5 text-blue-300" />
            Codex 多模型路由工作台
          </div>
          <p className="max-w-4xl text-sm leading-6 text-slate-300">
            这里配置的是“Codex 自己怎么按 model 选择多个上游模型”。Codex 仍然只连接一个
            CC Switch 本地代理；路由规则负责把 gpt、qwen、deepseek 等模型名分流到不同上游。
          </p>
          <div className="flex flex-wrap gap-2">
            <Button onClick={onCreatePlan} className="gap-2 bg-blue-600 hover:bg-blue-500">
              <Plus className="h-4 w-4" />
              创建多路路由
            </Button>
            <Button variant="outline" onClick={() => onJump("routes")} className="gap-2">
              <Settings2 className="h-4 w-4" />
              管理路由规则
            </Button>
            <Button variant="outline" onClick={() => onJump("test")} className="gap-2">
              <Play className="h-4 w-4" />
              预览匹配结果
            </Button>
          </div>
        </div>

        <div className="grid gap-3 sm:grid-cols-2">
          <MetricCard
            color="blue"
            icon={Layers3}
            label="多路路由"
            value={`${routingPlans.length} 个`}
            detail="可直接承载 Codex 多模型分流"
          />
          <MetricCard
            color="emerald"
            icon={Route}
            label="启用规则"
            value={`${enabledRoutes.length} / ${routeEntries.length}`}
            detail="匹配请求里的 model"
          />
          <MetricCard
            color="amber"
            icon={Server}
            label="可接入模型源"
            value={`${modelSources.length} 个`}
            detail="可编辑后加入路由方案"
          />
          <MetricCard
            color="rose"
            icon={ShieldCheck}
            label="隔离策略"
            value="不接管"
            detail="不修改 Codex 当前 Provider"
          />
        </div>
      </div>

      <div className="grid gap-3 p-4 md:grid-cols-4">
        <FlowStep index="1" title="模型源" detail="准备 OpenAI、Qwen、DeepSeek 等上游" />
        <FlowStep index="2" title="多路路由" detail="把多个上游收进一个 Codex 入口" />
        <FlowStep index="3" title="匹配规则" detail="按精确模型名或前缀分流" />
        <FlowStep index="4" title="测试发布" detail="预览 model 会命中哪条规则" />
      </div>
    </div>
  );
}

/// 选项卡触发器封装，统一图标和可点击态。
function WorkspaceTabTrigger({
  value,
  icon: Icon,
  label,
}: {
  value: WorkspaceTab;
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

/// 总览页展示当前方案、关键规则和下一步动作，避免用户只看到一堆不可操作卡片。
function OverviewTab({
  routingPlans,
  routeEntries,
  modelSources,
  onCreatePlan,
  onSelectPlan,
  onSelectRoute,
  onJump,
}: {
  routingPlans: Provider[];
  routeEntries: RouteEntry[];
  modelSources: Provider[];
  onCreatePlan: () => void;
  onSelectPlan: (provider: Provider) => void;
  onSelectRoute: (entry: RouteEntry) => void;
  onJump: (tab: WorkspaceTab) => void;
}) {
  return (
    <div className="grid gap-4 xl:grid-cols-[1.05fr_0.95fr]">
      <section className="rounded-lg border border-blue-700/40 bg-blue-950/15 p-4">
        <SectionHeader
          icon={Layers3}
          title="多路路由"
          detail="每个多路路由都是一个 Codex 可连接的本地代理入口。"
          action={
            <Button size="sm" onClick={onCreatePlan} className="gap-2 bg-blue-600 hover:bg-blue-500">
              <Plus className="h-4 w-4" />
              创建多路路由
            </Button>
          }
        />
        <div className="mt-3 grid gap-3">
          {routingPlans.length === 0 ? (
            <EmptyState
              icon={Wand2}
              title="还没有多路路由"
              detail="先创建一个多路路由，再把多个模型源挂到它下面。"
              actionLabel="创建多路路由"
              onAction={onCreatePlan}
            />
          ) : (
            routingPlans.map((provider) => (
              <button
                key={provider.id}
                type="button"
                onClick={() => onSelectPlan(provider)}
                className="group rounded-lg border border-blue-600/40 bg-slate-950/40 p-4 text-left transition hover:border-blue-400 hover:bg-blue-950/30 hover:shadow-[0_0_0_1px_rgba(96,165,250,0.35)]"
              >
                <PlanCardContent provider={provider} />
              </button>
            ))
          )}
        </div>
      </section>

      <section className="rounded-lg border border-emerald-700/40 bg-emerald-950/10 p-4">
        <SectionHeader
          icon={Route}
          title="最近路由规则"
          detail="点击规则可以进入详情和测试。"
          action={
            <Button size="sm" variant="outline" onClick={() => onJump("routes")} className="gap-2">
              查看全部
              <ArrowRight className="h-4 w-4" />
            </Button>
          }
        />
        <div className="mt-3 grid gap-2">
          {routeEntries.slice(0, 4).map((entry) => (
            <RouteListButton
              key={`${entry.provider.id}-${entry.route.id ?? entry.index}`}
              entry={entry}
              onClick={() => onSelectRoute(entry)}
            />
          ))}
          {routeEntries.length === 0 && (
            <EmptyState
              icon={Route}
              title="还没有规则"
              detail="创建多路路由后，在编辑表单里添加模型匹配规则。"
              actionLabel="创建多路路由"
              onAction={onCreatePlan}
            />
          )}
        </div>
      </section>

      <section className="rounded-lg border border-amber-700/40 bg-amber-950/10 p-4 xl:col-span-2">
        <SectionHeader
          icon={Server}
          title="可接入模型源"
          detail="这些不是单独一类难懂的 Provider，而是可以被路由方案接入的上游模型源。"
          action={
            <Button size="sm" variant="outline" onClick={() => onJump("sources")}>
              选择模型源
            </Button>
          }
        />
        <div className="mt-3 grid gap-3 md:grid-cols-2 xl:grid-cols-4">
          {modelSources.slice(0, 8).map((provider) => (
            <SourceMiniCard key={provider.id} provider={provider} />
          ))}
        </div>
      </section>
    </div>
  );
}

/// 模型源页展示可被纳入路由的上游，并把“编辑后接入”作为明确操作。
function SourcesTab({
  modelSources,
  routingPlans,
  onCreatePlan,
  onEditPlan,
  onSelectPlan,
}: {
  modelSources: Provider[];
  routingPlans: Provider[];
  onCreatePlan: () => void;
  onEditPlan: (provider: Provider, detail?: string) => void;
  onSelectPlan: (provider: Provider) => void;
}) {
  return (
    <div className="grid gap-4 xl:grid-cols-[0.8fr_1.2fr]">
      <section className="rounded-lg border border-blue-700/40 bg-blue-950/15 p-4">
        <SectionHeader
          icon={Layers3}
          title="多路路由方案"
          detail="这是 Codex 最终连接的路由入口；选择后到“路由规则”里挂接模型源。"
          action={
            <Button size="sm" onClick={onCreatePlan} className="gap-2 bg-blue-600 hover:bg-blue-500">
              <Plus className="h-4 w-4" />
              创建多路路由
            </Button>
          }
        />
        <div className="mt-3 grid gap-2">
          {routingPlans.map((provider) => (
            <button
              key={provider.id}
              type="button"
              onClick={() => onSelectPlan(provider)}
              className="rounded-lg border border-blue-700/40 bg-slate-950/40 p-3 text-left transition hover:border-blue-400 hover:bg-blue-950/30"
            >
              <PlanCardContent provider={provider} compact />
            </button>
          ))}
          {routingPlans.length === 0 && (
            <EmptyState
              icon={Layers3}
              title="还没有多路路由"
              detail="先创建一个 Codex 多模型路由入口，再选择模型源接入。"
              actionLabel="创建多路路由"
              onAction={onCreatePlan}
            />
          )}
        </div>
      </section>

      <section className="rounded-lg border border-amber-700/40 bg-amber-950/10 p-4">
        <SectionHeader
          icon={Server}
          title="选择模型源"
          detail="这里选择要接入多路路由的上游模型源；点卡片进入模型源配置。"
        />
        <div className="mt-3 grid gap-3 md:grid-cols-2">
          {modelSources.map((provider) => (
            <button
              key={provider.id}
              type="button"
              onClick={() => onEditPlan(provider, "选择并编辑模型源，准备接入多路路由")}
              className="group rounded-lg border border-amber-700/40 bg-slate-950/40 p-4 text-left transition hover:border-amber-400 hover:bg-amber-950/20 hover:shadow-[0_0_0_1px_rgba(251,191,36,0.25)]"
            >
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0">
                  <div className="truncate text-sm font-semibold text-slate-100">
                    {provider.name}
                  </div>
                  <div className="mt-1 truncate text-xs text-slate-400">
                    ID：{provider.id}
                  </div>
                </div>
                <Badge className="border-amber-500/50 bg-amber-500/15 text-amber-100">
                  可选
                </Badge>
              </div>
              <div className="mt-4 flex items-center justify-between text-xs">
                <span className="text-slate-400">选择这个模型源</span>
                <span className="inline-flex items-center gap-1 text-amber-200 opacity-80 group-hover:opacity-100">
                  选择
                  <Pencil className="h-3.5 w-3.5" />
                </span>
              </div>
            </button>
          ))}
        </div>
      </section>
    </div>
  );
}

/// 路由规则页提供方案选择、规则列表和右侧详情，形成真实的“查/改/删入口”工作流。
function RoutesTab({
  routingPlans,
  routeEntries,
  selectedPlan,
  selectedRoute,
  onCreatePlan,
  onEditPlan,
  onSelectPlan,
  onSelectRoute,
}: {
  routingPlans: Provider[];
  routeEntries: RouteEntry[];
  selectedPlan: Provider | null;
  selectedRoute?: RouteEntry;
  onCreatePlan: () => void;
  onEditPlan: (provider: Provider, detail?: string) => void;
  onSelectPlan: (provider: Provider) => void;
  onSelectRoute: (entry: RouteEntry) => void;
}) {
  const selectedPlanRoutes = selectedPlan
    ? routeEntries.filter(({ provider }) => provider.id === selectedPlan.id)
    : routeEntries;

  return (
    <div className="grid gap-4 xl:grid-cols-[360px_1fr]">
      <section className="rounded-lg border border-blue-700/40 bg-blue-950/15 p-4">
        <SectionHeader
          icon={Layers3}
          title="选择多路路由"
          detail="每个多路路由可包含多条分流规则。"
          action={
            <Button size="sm" onClick={onCreatePlan} className="gap-2 bg-blue-600 hover:bg-blue-500">
              <Plus className="h-4 w-4" />
              创建多路路由
            </Button>
          }
        />
        <div className="mt-3 grid gap-2">
          {routingPlans.map((provider) => {
            const active = selectedPlan?.id === provider.id;
            return (
              <button
                key={provider.id}
                type="button"
                onClick={() => onSelectPlan(provider)}
                className={cn(
                  "rounded-lg border p-3 text-left transition",
                  active
                    ? "border-blue-400 bg-blue-600/20 shadow-[0_0_0_1px_rgba(96,165,250,0.35)]"
                    : "border-slate-700 bg-slate-950/40 hover:border-blue-500 hover:bg-blue-950/20",
                )}
              >
                <PlanCardContent provider={provider} compact />
              </button>
            );
          })}
        </div>
      </section>

      <section className="grid gap-4 lg:grid-cols-[1fr_360px]">
        <div className="rounded-lg border border-emerald-700/40 bg-emerald-950/10 p-4">
          <SectionHeader
            icon={Route}
            title="规则列表"
            detail="点击规则查看详情；增删改规则在右上角编辑多路路由里完成。"
            action={
              selectedPlan ? (
                <Button
                  size="sm"
                  onClick={() => onEditPlan(selectedPlan, "添加、修改或删除路由规则")}
                  className="gap-2 bg-emerald-600 hover:bg-emerald-500"
                >
                  <Pencil className="h-4 w-4" />
                  编辑规则
                </Button>
              ) : null
            }
          />
          <div className="mt-3 grid gap-2">
            {selectedPlanRoutes.map((entry) => (
              <RouteListButton
                key={`${entry.provider.id}-${entry.route.id ?? entry.index}`}
                entry={entry}
                active={selectedRoute?.provider.id === entry.provider.id && selectedRoute.index === entry.index}
                onClick={() => onSelectRoute(entry)}
              />
            ))}
            {selectedPlanRoutes.length === 0 && (
              <EmptyState
                icon={Route}
                title="这个方案还没有规则"
              detail="点击编辑规则，在配置表单里添加精确模型或前缀匹配。"
                actionLabel="编辑多路路由"
                onAction={() => (selectedPlan ? onEditPlan(selectedPlan) : onCreatePlan())}
              />
            )}
          </div>
        </div>

        <RouteDetailPanel
          selectedRoute={selectedRoute}
          selectedPlan={selectedPlan}
          onEditPlan={onEditPlan}
        />
      </section>
    </div>
  );
}

/// 测试发布页只做本地匹配预览，并展示下一步如何发布到 Codex。
function TestTab({
  selectedPlan,
  selectedRouting,
  routeModels,
  testModel,
  testResult,
  onModelChange,
  onPreviewRoute,
  onEditPlan,
}: {
  selectedPlan: Provider | null;
  selectedRouting: CodexRouting | null;
  routeModels: string[];
  testModel: string;
  testResult: string | null;
  onModelChange: (value: string) => void;
  onPreviewRoute: () => void;
  onEditPlan: (provider: Provider, detail?: string) => void;
}) {
  return (
    <div className="grid gap-4 xl:grid-cols-[1fr_420px]">
      <section className="rounded-lg border border-purple-700/40 bg-purple-950/10 p-4">
        <SectionHeader
          icon={Play}
          title="匹配预览"
          detail="输入 Codex 请求中的 model，先在本地预览会命中哪条规则。"
        />
        <div className="mt-4 grid gap-3 md:grid-cols-[1fr_auto]">
          <input
            value={testModel}
            onChange={(event) => onModelChange(event.target.value)}
            placeholder="例如：gpt-5.4-mini、qwen3.6、deepseek-v4-flash"
            className="h-10 rounded-md border border-purple-700/50 bg-slate-950/70 px-3 text-sm outline-none transition placeholder:text-slate-500 focus:border-purple-400 focus:ring-2 focus:ring-purple-500/30"
          />
          <Button onClick={onPreviewRoute} className="gap-2 bg-purple-600 hover:bg-purple-500">
            <Play className="h-4 w-4" />
            预览命中
          </Button>
        </div>
        {routeModels.length > 0 && (
          <div className="mt-3 flex flex-wrap gap-2">
            {routeModels.slice(0, 10).map((model) => (
              <button
                key={model}
                type="button"
                onClick={() => onModelChange(model.replace(/\*$/, ""))}
                className="rounded-full border border-purple-500/40 bg-purple-500/10 px-3 py-1 text-xs text-purple-100 transition hover:border-purple-300 hover:bg-purple-500/20"
              >
                {model}
              </button>
            ))}
          </div>
        )}
        <div className="mt-4 rounded-lg border border-purple-700/40 bg-slate-950/50 p-4">
          <div className="mb-2 flex items-center gap-2 text-sm font-semibold">
            <Activity className="h-4 w-4 text-purple-300" />
            预览结果
          </div>
          <p className="text-sm leading-6 text-slate-300">
            {testResult ?? "还没有执行预览。这里不会请求真实上游，也不会消耗额度。"}
          </p>
        </div>
      </section>

      <section className="rounded-lg border border-emerald-700/40 bg-emerald-950/10 p-4">
        <SectionHeader
          icon={RadioTower}
          title="发布检查"
          detail="确认后再到配置表单保存。"
          action={
            selectedPlan ? (
              <Button
                size="sm"
                onClick={() => onEditPlan(selectedPlan, "打开发布前配置检查")}
                className="gap-2 bg-emerald-600 hover:bg-emerald-500"
              >
                <Pencil className="h-4 w-4" />
                编辑多路路由
              </Button>
            ) : null
          }
        />
        <div className="mt-4 space-y-3">
          <ChecklistItem ok={Boolean(selectedPlan)} label="已选择多路路由" />
          <ChecklistItem ok={selectedRouting?.enabled !== false} label="多路路由处于启用状态" />
          <ChecklistItem ok={Boolean(selectedRouting?.defaultRouteId)} label="已设置默认路由" />
          <ChecklistItem ok={(selectedRouting?.routes?.length ?? 0) > 0} label="至少有一条路由规则" />
          <ChecklistItem ok label="不会切换 Codex 当前 Provider" />
        </div>
      </section>
    </div>
  );
}

/// 操作记录页提供本次页面内的增删改查痕迹，让工作台不再像静态说明页。
function RecordsTab({
  records,
  onCreatePlan,
  onClear,
}: {
  records: RouteRecord[];
  onCreatePlan: () => void;
  onClear: () => void;
}) {
  return (
    <section className="rounded-lg border border-slate-700 bg-slate-950/40 p-4">
      <SectionHeader
        icon={FileClock}
        title="操作记录"
        detail="记录当前页面的查看、创建、编辑和测试动作；真实配置仍保存在模型源数据里。"
        action={
          <div className="flex gap-2">
            <Button size="sm" variant="outline" onClick={onClear} className="gap-2">
              <Trash2 className="h-4 w-4" />
              清空临时记录
            </Button>
            <Button size="sm" onClick={onCreatePlan} className="gap-2 bg-blue-600 hover:bg-blue-500">
              <Plus className="h-4 w-4" />
              创建多路路由
            </Button>
          </div>
        }
      />
      <div className="mt-4 overflow-hidden rounded-lg border border-slate-700">
        {records.map((record) => (
          <div
            key={record.id}
            className="grid gap-2 border-b border-slate-800 bg-slate-950/40 p-3 text-sm last:border-b-0 md:grid-cols-[120px_120px_1fr]"
          >
            <div className="text-slate-400">{record.time}</div>
            <div className="font-semibold text-slate-100">{record.action}</div>
            <div className="text-slate-300">{record.detail}</div>
          </div>
        ))}
      </div>
    </section>
  );
}

/// 指标卡使用不同主题色，帮助用户快速区分状态而不是看到一片灰。
function MetricCard({
  color,
  icon: Icon,
  label,
  value,
  detail,
}: {
  color: "blue" | "emerald" | "amber" | "rose";
  icon: React.ComponentType<{ className?: string }>;
  label: string;
  value: string;
  detail: string;
}) {
  const styles = {
    blue: "border-blue-500/40 bg-blue-500/10 text-blue-200",
    emerald: "border-emerald-500/40 bg-emerald-500/10 text-emerald-200",
    amber: "border-amber-500/40 bg-amber-500/10 text-amber-200",
    rose: "border-rose-500/40 bg-rose-500/10 text-rose-200",
  }[color];

  return (
    <div className={cn("rounded-lg border p-3", styles)}>
      <div className="flex items-center justify-between gap-2">
        <span className="text-xs opacity-80">{label}</span>
        <Icon className="h-4 w-4 opacity-80" />
      </div>
      <div className="mt-2 text-2xl font-semibold text-white">{value}</div>
      <div className="mt-1 text-xs opacity-75">{detail}</div>
    </div>
  );
}

/// 流程步骤用于解释这套逻辑如何从模型源变成 Codex 可用的多模型入口。
function FlowStep({
  index,
  title,
  detail,
}: {
  index: string;
  title: string;
  detail: string;
}) {
  return (
    <div className="rounded-lg border border-slate-700 bg-slate-950/40 p-3">
      <div className="flex items-center gap-2">
        <span className="grid h-6 w-6 place-items-center rounded-full bg-blue-600 text-xs font-bold text-white">
          {index}
        </span>
        <span className="text-sm font-semibold text-slate-100">{title}</span>
      </div>
      <div className="mt-2 text-xs leading-5 text-slate-400">{detail}</div>
    </div>
  );
}

/// 通用标题行，统一不同页面区块的操作按钮位置。
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

/// 路由方案卡片内容；外层决定是按钮还是静态容器。
function PlanCardContent({
  provider,
  compact = false,
}: {
  provider: Provider;
  compact?: boolean;
}) {
  const routing = readCodexRouting(provider);
  const routes = routing?.routes ?? [];

  return (
    <div className="min-w-0">
      <div className="flex flex-wrap items-center gap-2">
        <span className="truncate font-semibold text-slate-100">{provider.name}</span>
        <Badge
          className={cn(
            "border",
            routing?.enabled === false
              ? "border-slate-500/50 bg-slate-500/10 text-slate-200"
              : "border-emerald-500/50 bg-emerald-500/15 text-emerald-100",
          )}
        >
          {routing?.enabled === false ? "已停用" : "已启用"}
        </Badge>
      </div>
      <div className="mt-2 flex flex-wrap gap-2 text-xs text-slate-400">
        <span>规则 {routes.length} 条</span>
        {routing?.defaultRouteId && <span>默认 {routing.defaultRouteId}</span>}
        {!compact && <span>ID {provider.id}</span>}
      </div>
    </div>
  );
}

/// 路由规则按钮，比普通卡片有更明显的 hover 和 active 态。
function RouteListButton({
  entry,
  active = false,
  onClick,
}: {
  entry: RouteEntry;
  active?: boolean;
  onClick: () => void;
}) {
  const format = routeApiFormat(entry.route);

  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "group rounded-lg border p-3 text-left transition",
        active
          ? "border-emerald-400 bg-emerald-600/20 shadow-[0_0_0_1px_rgba(52,211,153,0.3)]"
          : "border-slate-700 bg-slate-950/40 hover:border-emerald-400 hover:bg-emerald-950/20",
      )}
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="min-w-0">
          <div className="truncate text-sm font-semibold text-slate-100">
            {entry.route.label || entry.route.id || "未命名规则"}
          </div>
          <div className="mt-1 truncate text-xs text-slate-400">
            所属多路路由：{entry.provider.name}
          </div>
        </div>
        <Badge
          className={cn(
            "border",
            entry.route.enabled === false
              ? "border-slate-500/50 bg-slate-500/10 text-slate-200"
              : "border-emerald-500/50 bg-emerald-500/15 text-emerald-100",
          )}
        >
          {entry.route.enabled === false ? "停用" : "启用"}
        </Badge>
      </div>
      <div className="mt-3 flex flex-wrap gap-2 text-xs">
        <span className="rounded-full border border-blue-500/40 bg-blue-500/10 px-2 py-0.5 text-blue-100">
          {apiFormatLabel(format)}
        </span>
        <span className="rounded-full border border-slate-600 bg-slate-900 px-2 py-0.5 text-slate-300">
          {authSourceLabel(entry.route.upstream?.auth?.source)}
        </span>
      </div>
      <div className="mt-2 truncate text-xs text-slate-400">
        {routeMatchSummary(entry.route)}
      </div>
    </button>
  );
}

/// 右侧规则详情，把“查看、编辑、删除入口、复制模型名”分开展示，减少不可操作感。
function RouteDetailPanel({
  selectedRoute,
  selectedPlan,
  onEditPlan,
}: {
  selectedRoute?: RouteEntry;
  selectedPlan: Provider | null;
  onEditPlan: (provider: Provider, detail?: string) => void;
}) {
  if (!selectedRoute) {
    return (
      <section className="rounded-lg border border-slate-700 bg-slate-950/40 p-4">
        <EmptyState
          icon={Route}
          title="请选择一条规则"
          detail="左侧点击规则后，这里会展示上游、匹配条件和操作入口。"
          actionLabel={selectedPlan ? "编辑多路路由" : "创建多路路由"}
          onAction={() => selectedPlan && onEditPlan(selectedPlan)}
        />
      </section>
    );
  }

  const route = selectedRoute.route;
  const matchedModels = route.match?.models ?? [];

  return (
    <section className="rounded-lg border border-emerald-700/40 bg-slate-950/50 p-4">
      <SectionHeader
        icon={Database}
        title={route.label || route.id || "规则详情"}
        detail="这里是当前规则的只读摘要；修改和删除会进入配置表单。"
        action={
          <Button
            size="sm"
            onClick={() => onEditPlan(selectedRoute.provider, "编辑或删除当前路由规则")}
            className="gap-2 bg-emerald-600 hover:bg-emerald-500"
          >
            <Pencil className="h-4 w-4" />
            编辑
          </Button>
        }
      />
      <div className="mt-4 space-y-3 text-sm">
        <DetailRow label="匹配条件" value={routeMatchSummary(route)} />
        <DetailRow label="上游地址" value={routeBaseUrl(route)} />
        <DetailRow label="接口类型" value={apiFormatLabel(routeApiFormat(route))} />
        <DetailRow label="认证方式" value={authSourceLabel(route.upstream?.auth?.source)} />
        <DetailRow
          label="能力"
          value={[
            route.capabilities?.textOnly ? "仅文本" : "图文",
            route.capabilities?.supportsReasoning ? "推理" : null,
          ]
            .filter(Boolean)
            .join(" / ")}
        />
      </div>
      <div className="mt-4 grid gap-2">
        <Button
          variant="outline"
          className="justify-start gap-2"
          onClick={() => navigator.clipboard?.writeText(matchedModels.join(", "))}
          disabled={matchedModels.length === 0}
        >
          <Clipboard className="h-4 w-4" />
          复制精确模型名
        </Button>
        <Button
          variant="outline"
          className="justify-start gap-2 text-rose-200 hover:text-rose-100"
          onClick={() => onEditPlan(selectedRoute.provider, "打开表单后可删除当前规则")}
        >
          <Trash2 className="h-4 w-4" />
          删除入口在编辑表单中
        </Button>
      </div>
    </section>
  );
}

/// 只读详情行，避免信息散落成难扫描的长段落。
function DetailRow({ label, value }: { label: string; value?: string }) {
  return (
    <div className="rounded-md border border-slate-800 bg-slate-950/50 p-3">
      <div className="text-xs text-slate-500">{label}</div>
      <div className="mt-1 break-words text-slate-200">{value || "未配置"}</div>
    </div>
  );
}

/// 模型源迷你卡，仅用于总览页快速提示。
function SourceMiniCard({ provider }: { provider: Provider }) {
  return (
    <div className="rounded-lg border border-amber-700/30 bg-slate-950/40 p-3">
      <div className="truncate text-sm font-semibold text-slate-100">{provider.name}</div>
      <div className="mt-1 truncate text-xs text-slate-400">{provider.id}</div>
    </div>
  );
}

/// 发布检查项用色彩表达状态，避免所有信息都像普通文字。
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
      <CheckCircle2 className="h-4 w-4" />
      {label}
    </div>
  );
}

/// 空状态组件带明确动作按钮，让无数据场景仍可继续操作。
function EmptyState({
  icon: Icon,
  title,
  detail,
  actionLabel,
  onAction,
}: {
  icon: React.ComponentType<{ className?: string }>;
  title: string;
  detail: string;
  actionLabel: string;
  onAction?: () => void;
}) {
  return (
    <div className="rounded-lg border border-dashed border-slate-700 bg-slate-950/40 p-5">
      <div className="flex items-start gap-3">
        <Icon className="mt-0.5 h-5 w-5 text-slate-400" />
        <div className="min-w-0 flex-1">
          <div className="font-semibold text-slate-100">{title}</div>
          <p className="mt-1 text-sm leading-6 text-slate-400">{detail}</p>
          {onAction && (
            <Button size="sm" variant="outline" onClick={onAction} className="mt-3">
              {actionLabel}
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}
