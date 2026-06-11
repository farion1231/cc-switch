import {
  AlertTriangle,
  CheckCircle2,
  LockKeyhole,
  Route,
  Server,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { ExternalOpenAIAPIBackendOption } from "@/types/proxy";
import {
  describeBackendTarget,
  displayBackendDescription,
  type BackendGroup,
  type BackendTargetDescription,
} from "@/lib/openai/externalProfile";

/// 渲染可复用的服务来源卡片列表；不可用来源也可点击查看原因，但不能保存启用。
export function ExternalBackendPicker({
  groups,
  selectedKey,
  onSelect,
}: {
  groups: BackendGroup[];
  selectedKey: string;
  onSelect: (key: string) => void;
}) {
  if (groups.length === 0) {
    return (
      <div className="rounded-lg border border-dashed border-slate-700 bg-slate-950/40 p-5 text-sm text-slate-400">
        还没有可作为 OpenAI-compatible API 的服务来源。先添加 OpenAI-compatible
        模型源，或配置 OpenAI 官方登录。
      </div>
    );
  }

  return (
    <div className="space-y-4">
      {groups.map((group) => (
        <div key={group.key} className="space-y-2">
          <div className="flex items-center justify-between gap-3">
            <div className="text-xs font-semibold uppercase tracking-wide text-slate-400">
              {group.label}
            </div>
            <Badge className={groupBadgeClass(group.tone)}>
              {group.options.length} 个来源
            </Badge>
          </div>
          <div className="grid gap-2 xl:grid-cols-2">
            {group.options.map((option) => (
              <BackendSourceCard
                key={option.key}
                option={option}
                tone={group.tone}
                selected={option.key === selectedKey}
                onSelect={() => onSelect(option.key)}
              />
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

/// 渲染单个服务来源，用颜色和图标区分官方 OAuth、路由规则、普通模型源和不可用来源。
function BackendSourceCard({
  option,
  tone,
  selected,
  onSelect,
}: {
  option: ExternalOpenAIAPIBackendOption;
  tone: BackendGroup["tone"];
  selected: boolean;
  onSelect: () => void;
}) {
  const details = describeBackendTarget(option);
  const Icon = option.isManagedOAuth
    ? LockKeyhole
    : option.backendType === "codex_router_route"
      ? Route
      : option.available
        ? Server
        : AlertTriangle;

  return (
    <button
      type="button"
      onClick={onSelect}
      className={cn(
        "min-w-0 rounded-lg border p-3 text-left transition",
        cardToneClass(tone, selected, option.available),
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 gap-2">
          <span className={cn("mt-0.5 rounded-md p-1.5", iconToneClass(tone))}>
            <Icon className="h-4 w-4" />
          </span>
          <div className="min-w-0">
            <div className="truncate text-sm font-semibold text-slate-100">
              {option.label}
            </div>
            <div className="mt-1 truncate text-xs text-slate-400">
              {displayBackendDescription(option.description)}
            </div>
          </div>
        </div>
        {selected && <CheckCircle2 className="h-4 w-4 shrink-0 text-blue-300" />}
      </div>

      <div className="mt-3 flex flex-wrap gap-1.5">
        <Badge className={option.available ? "bg-emerald-500/15 text-emerald-100" : "bg-slate-500/15 text-slate-300"}>
          {option.available ? "可接入" : "需补配置"}
        </Badge>
        <Badge variant="outline">{details.kind}</Badge>
        <Badge variant="outline">{details.modelSource}</Badge>
      </div>

      {!option.available && option.error && (
        <div className="mt-2 rounded-md border border-amber-500/30 bg-amber-500/10 px-2 py-1.5 text-xs leading-5 text-amber-100">
          {translateBackendError(option.error)}
        </div>
      )}
    </button>
  );
}

/// 展示已选服务来源的摘要；命名改为“对外服务来源”，避免“后端目标”这种工程术语。
export function SelectedBackendSummary({
  backend,
  description,
  hasDraftChanges,
}: {
  backend?: ExternalOpenAIAPIBackendOption;
  description: BackendTargetDescription;
  hasDraftChanges: boolean;
}) {
  return (
    <div className="rounded-lg border border-blue-700/40 bg-blue-950/15 p-4">
      <div className="mb-3 flex items-center justify-between gap-2">
        <div className="text-sm font-semibold text-slate-100">对外服务来源</div>
        <Badge variant={hasDraftChanges ? "outline" : "secondary"}>
          {hasDraftChanges ? "待保存" : "已保存"}
        </Badge>
      </div>
      <div className="space-y-2 text-xs text-slate-400">
        <SummaryLine label="来源" value={backend?.label ?? "未选择"} />
        <SummaryLine label="类型" value={description.kind} />
        <SummaryLine label="路径" value={description.protocol} />
        <SummaryLine label="认证" value={description.auth} />
        <SummaryLine label="模型" value={description.modelSource} />
      </div>
      <div className="mt-3 flex flex-wrap gap-1.5">
        {description.compatibility.map((item) => (
          <Badge key={item} variant="outline">
            {item}
          </Badge>
        ))}
      </div>
      {backend && !backend.available && (
        <Button disabled className="mt-3 w-full">
          当前来源需要补配置，不能启用
        </Button>
      )}
    </div>
  );
}

/// 渲染摘要行，确保长名称不会撑破布局。
function SummaryLine({ label, value }: { label: string; value: string }) {
  return (
    <div className="grid grid-cols-[58px_minmax(0,1fr)] gap-2">
      <span>{label}</span>
      <span className="truncate text-slate-100">{value}</span>
    </div>
  );
}

function groupBadgeClass(tone: BackendGroup["tone"]): string {
  return {
    blue: "border-blue-500/40 bg-blue-500/10 text-blue-100",
    emerald: "border-emerald-500/40 bg-emerald-500/10 text-emerald-100",
    amber: "border-amber-500/40 bg-amber-500/10 text-amber-100",
    slate: "border-slate-500/40 bg-slate-500/10 text-slate-200",
  }[tone];
}

function iconToneClass(tone: BackendGroup["tone"]): string {
  return {
    blue: "bg-blue-500/15 text-blue-200",
    emerald: "bg-emerald-500/15 text-emerald-200",
    amber: "bg-amber-500/15 text-amber-200",
    slate: "bg-slate-500/15 text-slate-200",
  }[tone];
}

function cardToneClass(
  tone: BackendGroup["tone"],
  selected: boolean,
  available: boolean,
): string {
  if (!available) {
    return selected
      ? "border-slate-400 bg-slate-800/60"
      : "border-slate-700 bg-slate-950/40 hover:border-slate-500 hover:bg-slate-900/60";
  }
  const selectedClasses = {
    blue: "border-blue-400 bg-blue-600/20 shadow-[0_0_0_1px_rgba(96,165,250,0.35)]",
    emerald:
      "border-emerald-400 bg-emerald-600/20 shadow-[0_0_0_1px_rgba(52,211,153,0.35)]",
    amber:
      "border-amber-400 bg-amber-600/20 shadow-[0_0_0_1px_rgba(251,191,36,0.3)]",
    slate: "border-slate-400 bg-slate-800/60",
  }[tone];
  const idleClasses = {
    blue: "border-blue-700/40 bg-slate-950/40 hover:border-blue-400 hover:bg-blue-950/25",
    emerald:
      "border-emerald-700/40 bg-slate-950/40 hover:border-emerald-400 hover:bg-emerald-950/20",
    amber:
      "border-amber-700/40 bg-slate-950/40 hover:border-amber-400 hover:bg-amber-950/20",
    slate: "border-slate-700 bg-slate-950/40 hover:border-slate-500 hover:bg-slate-900/60",
  }[tone];
  return selected ? selectedClasses : idleClasses;
}

function translateBackendError(error: string): string {
  if (error.includes("native protocol")) {
    return "这是 Claude/Gemini 等原生协议配置，不能直接伪装成 OpenAI v1 API。";
  }
  if (error.includes("no usable base URL or credential")) {
    return "缺少可用的 Base URL 或凭据。补齐后才能作为第三方 Agent 的模型来源。";
  }
  if (error.includes("route needs managed OAuth")) {
    return "这条路由缺少托管 OAuth、路由 API Key 或可继承的模型源凭据。";
  }
  return error;
}
