import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import i18n from "@/i18n";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  Bot,
  CircleStop,
  Copy as CopyIcon,
  FileText,
  FolderOpen,
  Play,
  RefreshCw,
  RotateCcw,
  ShieldCheck,
  Skull,
  Terminal,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  agentGatewayApi,
  formatAgentError,
  type AgentInstance,
  type AgentLog,
  type AgentPermissionMode,
  type AgentRuntimeKind,
  type LaunchStrategy,
  type ProviderRuntimeSnapshot,
  type RunProfile,
} from "@/lib/api/agentGateway";
import { diagnosticsApi, type DiagnosticReport } from "@/lib/api/diagnostics";
import { useProvidersQuery } from "@/lib/query/queries";
import { sessionsApi } from "@/lib/api/sessions";
import type { SessionMeta } from "@/types";
import { cn } from "@/lib/utils";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

interface AgentsPanelProps {
  onOpenChange: (open: boolean) => void;
}

type UiLang = "zh" | "en";
type PermissionMode = AgentPermissionMode;
type ResumeMode = "new" | "resume_id";
type ModelMode = "provider_default" | "custom";
type LaunchMode = "auto" | LaunchStrategy;
type ProviderMode = "selected_provider" | "current_cc_switch_provider";

const copy = {
  zh: {
    title: "Agent Gateway",
    native: "原生兼容模式",
    intro:
      "默认沿用当前 CC Switch / Claude Code 配置，只注入本地代理地址和占位 Token；不会改写 .claude、MCP、Skills 或历史会话目录。",
    refresh: "刷新",
    doctor: "首次运行检查",
    doctorHint:
      "检查失败不会影响原 Provider / MCP / Skills / Sessions / Usage 页面；只会让 Agent Gateway 降级。",
    run: "检查",
    export: "导出诊断",
    launchTitle: "启动 Claude Code Agent",
    name: "名称",
    runtime: "运行时",
    provider: "供应商",
    modelMode: "模型来源",
    model: "模型名",
    permission: "权限等级",
    resumeMode: "会话恢复",
    sessionId: "会话 ID",
    projectPath: "项目目录",
    chooseDirectory: "选择目录",
    runProfile: "启动配置",
    launchMode: "启动窗口",
    monitor: "运行监控",
    monitorHint: "端口从 15722-15799 自动分配，并强制路由到选定供应商。",
    logs: "日志",
    selected: "选中 Agent",
    launch: "启动",
    noAgents: "暂无 Agent。选择供应商后点击启动。",
    noLogs: "暂无日志。",
    selectAgent: "选择一个 Agent 查看日志。",
    dataSafety: "数据安全",
    dataSafetyText:
      "MVP 不复制、不软链接、不重写 Claude/MCP/Skills/Session 配置。恢复会话仍走 Claude Code 原生逻辑。",
    commandPreview: "实际启动命令",
    commandPreviewHint:
      "下方只展示 Claude CLI 参数格式；应用还会额外注入 ANTHROPIC_BASE_URL 和 ANTHROPIC_AUTH_TOKEN。供应商实际模型不会直接塞给 claude --model，而是由本地代理按供应商配置映射。",
    upstreamModel: "供应商实际模型",
    upstreamModelHint:
      "启动前会用该供应商和实际模型做一次真实输出测试；测试不通过不会启动 Claude Code。",
    entryModelHint:
      "只有 claude-* 入口模型才会传给 claude --model；DeepSeek/MiniMax 等上游模型交给代理映射，避免 400 参数错误。",
    safeDesc: "默认权限：直接运行 claude，保留 Claude Code 官方默认权限确认。",
    planDesc: "计划模式：先规划再执行，适合审查改动范围。",
    acceptEditsDesc:
      "自动接受编辑：允许编辑类操作自动通过，仍保留更高风险确认。",
    autoDesc: "自动模式：按 Claude Code 官方 auto 权限模式运行。",
    dontAskDesc: "不询问模式：按官方 dontAsk 权限模式运行，风险更高。",
    dangerDesc:
      "跳过权限：使用官方危险参数，适合你明确信任当前任务时手动选择。",
    launchAuto: "自动选择（推荐）",
    launchWt: "Windows Terminal 标签页",
    launchPs: "PowerShell 窗口",
    launchBg: "后台启动（少弹窗）",
    launchHint:
      "Claude Code 是交互式 CLI；要真正使用请选择 Windows Terminal 或 PowerShell。后台启动不会显示输入输出，只适合诊断。",
    newSession: "新会话",
    resumeById: "按会话 ID 恢复",
    providerDefault: "使用供应商当前模型",
    customModel: "手动填写模型",
    providerMode: "Provider 来源",
    selectedProviderMode: "使用选中的供应商",
    currentProviderMode: "使用当前 CC Switch 供应商",
    snapshotTitle: "Provider Runtime Snapshot",
    sourceDb: "来源：CC Switch Provider DB",
    nativeModeLine: "Claude Setting Mode：原生兼容；CLAUDE_CONFIG_DIR：not set",
    snapshotHint:
      "Agent Gateway 不读取本地 Claude setting 判断供应商；它使用 CC Switch Provider DB 中的选中供应商配置，并通过本地代理端口强制路由。",
    apiKeyConfigured: "API Key 已配置",
    apiKeyMissing: "API Key 缺失",
    baseUrlMissing: "Base URL 缺失",
    chooseSession: "选择会话",
    sessionPickerTitle: "选择 Claude Code 会话",
    noClaudeSessions: "没有找到 Claude Code 历史会话。",
    select: "选择",
    createdAt: "创建时间",
    lastActiveAt: "最后活跃",
    duration: "运行时长",
    deleteDisabled: "请先停止或强制终止后再删除记录。",
    optional: "可选",
    later: "后续支持",
    language: "语言",
    successLaunch: "Claude Code Agent 已启动",
    successStop: "Agent 已停止",
    successKill: "Agent 进程树已强制清理",
    successRestart: "Agent 已用原 ID 重启",
    exportSuccess: "诊断报告已导出",
    status: "状态",
    port: "端口",
    pid: "PID",
    actions: "操作",
    resume: "恢复",
    delete: "删除",
    copy: "复制",
    launchModeTag: "模式",
    newMode: "新会话",
    resumeModeTag: "恢复",
    providerId: "供应商 ID",
    agentId: "Agent ID",
    ccsaId: "CCSA 窗口 ID",
    localProxyUrl: "本地代理地址",
    cwd: "目录",
    lastError: "最后错误",
    startedAt: "启动时间",
    stoppedAt: "停止时间",
    sessionHelp:
      "这里需要 Claude Code session_id，不是 Agent ID，也不是 CCSA ID。",
    deleteConfirm:
      "只删除 Agent Gateway 记录，不会删除 Claude 会话或 .claude 配置。确认删除？",
    successDelete: "Agent 记录已隐藏",
  },
  en: {
    title: "Agent Gateway",
    native: "Native Compatibility",
    intro:
      "Uses current CC Switch / Claude Code behavior. It injects only local proxy URL and placeholder token; it does not rewrite .claude, MCP, Skills, or session storage.",
    refresh: "Refresh",
    doctor: "First Run Doctor",
    doctorHint:
      "Failures do not block Provider / MCP / Skills / Sessions / Usage pages; only Agent Gateway is degraded.",
    run: "Run",
    export: "Export",
    launchTitle: "Launch Claude Code Agent",
    name: "Name",
    runtime: "Runtime",
    provider: "Provider",
    modelMode: "Model source",
    model: "Model name",
    permission: "Permission level",
    resumeMode: "Conversation",
    sessionId: "Session ID",
    projectPath: "Project path",
    chooseDirectory: "Choose folder",
    runProfile: "Run profile",
    launchMode: "Launch window",
    monitor: "Runtime Monitor",
    monitorHint:
      "Ports are allocated from 15722-15799 and forced to the selected provider.",
    logs: "Logs",
    selected: "Selected Agent",
    launch: "Launch",
    noAgents: "No agents yet. Select a provider and launch.",
    noLogs: "No logs.",
    selectAgent: "Select an agent to view logs.",
    dataSafety: "Data safety",
    dataSafetyText:
      "MVP does not copy, symlink, or rewrite Claude/MCP/Skills/Session config. Resume uses native Claude Code behavior.",
    commandPreview: "Launch command",
    commandPreviewHint:
      "This shows Claude CLI arguments only; the app also injects ANTHROPIC_BASE_URL and ANTHROPIC_AUTH_TOKEN. Upstream provider models are not passed directly to claude --model; the local proxy maps them from provider config.",
    upstreamModel: "Provider upstream model",
    upstreamModelHint:
      "Before launch, the backend runs a real output test with this provider and upstream model. Claude Code will not launch if the test fails.",
    entryModelHint:
      "Only claude-* entry models are passed to claude --model. DeepSeek/MiniMax upstream IDs are routed by the proxy to avoid 400 parameter errors.",
    safeDesc:
      "Default: run claude and keep Claude Code's official permission behavior.",
    planDesc: "Plan mode: plan first before execution.",
    acceptEditsDesc:
      "Accept edits: automatically accept edit operations while keeping higher-risk prompts.",
    autoDesc: "Auto mode: run Claude Code's official auto permission mode.",
    dontAskDesc:
      "Don't ask: run Claude Code's official dontAsk mode. Higher risk.",
    dangerDesc:
      "Bypass permissions: uses the official dangerous flag. Select only for trusted tasks.",
    launchAuto: "Auto (Recommended)",
    launchWt: "Windows Terminal tab",
    launchPs: "PowerShell window",
    launchBg: "Background process",
    launchHint:
      "Claude Code is interactive. Use Windows Terminal or PowerShell for real work. Background hides all input/output and is mainly for diagnostics.",
    newSession: "New session",
    resumeById: "Resume by session ID",
    providerDefault: "Use provider current model",
    customModel: "Custom model",
    providerMode: "Provider source",
    selectedProviderMode: "Use selected provider",
    currentProviderMode: "Use current CC Switch provider",
    snapshotTitle: "Provider Runtime Snapshot",
    sourceDb: "Source: CC Switch Provider DB",
    nativeModeLine:
      "Claude Setting Mode: Native compatibility; CLAUDE_CONFIG_DIR: not set",
    snapshotHint:
      "Agent Gateway does not read local Claude settings to choose providers. It uses the selected provider from the CC Switch Provider DB and forces routing through the local listener.",
    apiKeyConfigured: "API key configured",
    apiKeyMissing: "API key missing",
    baseUrlMissing: "Base URL missing",
    chooseSession: "Choose session",
    sessionPickerTitle: "Choose Claude Code session",
    noClaudeSessions: "No Claude Code sessions found.",
    select: "Select",
    createdAt: "Created At",
    lastActiveAt: "Last Active",
    duration: "Duration",
    deleteDisabled: "Stop or kill the agent before deleting the record.",
    optional: "Optional",
    later: "later",
    language: "Language",
    successLaunch: "Claude Code agent launched",
    successStop: "Agent stopped",
    successKill: "Agent process tree killed",
    successRestart: "Agent restarted with the same ID",
    exportSuccess: "Diagnostic report exported",
    status: "Status",
    port: "Port",
    pid: "PID",
    actions: "Actions",
    resume: "Resume",
    delete: "Delete",
    copy: "Copy",
    launchModeTag: "Mode",
    newMode: "New",
    resumeModeTag: "Resume",
    providerId: "Provider ID",
    agentId: "Agent ID",
    ccsaId: "CCSA Window ID",
    localProxyUrl: "Local Proxy URL",
    cwd: "CWD",
    lastError: "Last Error",
    startedAt: "Started At",
    stoppedAt: "Stopped At",
    sessionHelp:
      "Use a Claude Code session_id here, not an Agent ID or CCSA ID.",
    deleteConfirm:
      "This only hides the Agent Gateway record. It will not delete Claude sessions or .claude config. Delete it?",
    successDelete: "Agent record hidden",
  },
};

const RUNTIME_OPTIONS: Array<{
  value: AgentRuntimeKind;
  zh: string;
  en: string;
  disabled?: boolean;
}> = [
  { value: "claude_code", zh: "Claude Code", en: "Claude Code" },
  { value: "codex", zh: "Codex（后续）", en: "Codex (later)", disabled: true },
  {
    value: "opencode",
    zh: "OpenCode（后续）",
    en: "OpenCode (later)",
    disabled: true,
  },
  {
    value: "open_claw",
    zh: "OpenClaw（后续）",
    en: "OpenClaw (later)",
    disabled: true,
  },
  {
    value: "gemini",
    zh: "Gemini（后续）",
    en: "Gemini (later)",
    disabled: true,
  },
];

const permissionOptions: Array<{
  value: PermissionMode;
  zh: string;
  en: string;
  commandArg: string;
}> = [
  {
    value: "default",
    zh: "默认权限（推荐）",
    en: "Default (Recommended)",
    commandArg: "",
  },
  {
    value: "plan",
    zh: "计划模式",
    en: "Plan mode",
    commandArg: "--permission-mode plan",
  },
  {
    value: "accept_edits",
    zh: "自动接受编辑",
    en: "Accept edits",
    commandArg: "--permission-mode acceptEdits",
  },
  {
    value: "auto",
    zh: "自动模式",
    en: "Auto mode",
    commandArg: "--permission-mode auto",
  },
  {
    value: "dont_ask",
    zh: "不询问模式",
    en: "Don't ask",
    commandArg: "--permission-mode dontAsk",
  },
  {
    value: "bypass_permissions",
    zh: "跳过权限（危险）",
    en: "Bypass permissions (dangerous)",
    commandArg: "--dangerously-skip-permissions",
  },
];

const statusClassName: Record<string, string> = {
  running: "border-emerald-500/40 bg-emerald-500/10 text-emerald-700",
  launching: "border-sky-500/40 bg-sky-500/10 text-sky-700",
  stopped: "border-zinc-500/40 bg-zinc-500/10 text-zinc-700",
  failed: "border-red-500/40 bg-red-500/10 text-red-700",
  killed: "border-red-500/40 bg-red-500/10 text-red-700",
  exited: "border-amber-500/40 bg-amber-500/10 text-amber-700",
};

const builtInProfiles = (lang: UiLang): RunProfile[] => [
  {
    id: "safe",
    name: lang === "zh" ? "安全启动" : "Safe",
    runtime: "claude_code",
    kind: "safe",
    args: [],
    env: [],
    allowCustomProfiles: false,
    createdAt: "",
    updatedAt: "",
  },
  {
    id: "resume",
    name: lang === "zh" ? "恢复会话" : "Resume",
    runtime: "claude_code",
    kind: "resume",
    args: ["--resume"],
    env: [],
    allowCustomProfiles: false,
    createdAt: "",
    updatedAt: "",
  },
];

const extractProviderModels = (settingsConfig: unknown): string[] => {
  if (!settingsConfig || typeof settingsConfig !== "object") return [];
  const config = settingsConfig as Record<string, unknown>;
  const env =
    config.env && typeof config.env === "object"
      ? (config.env as Record<string, unknown>)
      : config;
  const keys = [
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
  ];
  const seen = new Set<string>();
  const models: string[] = [];
  for (const key of keys) {
    const value = env[key];
    if (typeof value === "string" && value.trim() && !seen.has(value.trim())) {
      seen.add(value.trim());
      models.push(value.trim());
    }
  }
  return models;
};

const isClaudeCodeEntryModel = (model: string): boolean => {
  const normalized = model
    .trim()
    .replace(/\s*\[1m\]\s*$/i, "")
    .toLowerCase();
  return (
    normalized.startsWith("claude-") ||
    normalized.startsWith("anthropic/claude-")
  );
};

const terminalStatuses = new Set(["stopped", "failed", "exited", "killed"]);

const shortId = (value?: string | null): string => {
  if (!value) return "-";
  return value.length > 18
    ? `${value.slice(0, 8)}...${value.slice(-6)}`
    : value;
};

const formatTime = (value?: string | number | null): string => {
  if (!value) return "-";
  const date = typeof value === "number" ? new Date(value) : new Date(value);
  return Number.isNaN(date.getTime()) ? String(value) : date.toLocaleString();
};

const formatDuration = (
  started?: string | null,
  stopped?: string | null,
): string => {
  if (!started) return "-";
  const start = new Date(started).getTime();
  const end = stopped ? new Date(stopped).getTime() : Date.now();
  if (Number.isNaN(start) || Number.isNaN(end) || end < start) return "-";
  const seconds = Math.floor((end - start) / 1000);
  const minutes = Math.floor(seconds / 60);
  const hours = Math.floor(minutes / 60);
  if (hours > 0) return `${hours}h ${minutes % 60}m`;
  if (minutes > 0) return `${minutes}m ${seconds % 60}s`;
  return `${seconds}s`;
};

export function AgentsPanel({}: AgentsPanelProps) {
  const { t: globalT } = useTranslation();
  const initialLang: UiLang = i18n.language?.startsWith("en") ? "en" : "zh";
  const [lang, setLang] = useState<UiLang>(initialLang);
  const tt = copy[lang];

  const { data: providersData, isLoading: providersLoading } =
    useProvidersQuery("claude");
  const providers = useMemo(
    () => Object.values(providersData?.providers ?? {}),
    [providersData?.providers],
  );
  const providerNameById = useMemo(() => {
    return new Map(providers.map((provider) => [provider.id, provider.name]));
  }, [providers]);

  const [agents, setAgents] = useState<AgentInstance[]>([]);
  const [runProfiles, setRunProfiles] = useState<RunProfile[]>([]);
  const [selectedAgentId, setSelectedAgentId] = useState<string>("");
  const [logs, setLogs] = useState<AgentLog[]>([]);
  const [loading, setLoading] = useState(false);
  const [groupByFolder, setGroupByFolder] = useState(false);
  const [selectedAgentIds, setSelectedAgentIds] = useState<Set<string>>(
    new Set(),
  );
  const [diagnosticsLoading, setDiagnosticsLoading] = useState(false);
  const [diagnosticsReport, setDiagnosticsReport] =
    useState<DiagnosticReport | null>(null);
  const [actionId, setActionId] = useState<string | null>(null);
  const [lastError, setLastError] = useState<string | null>(null);
  const [permissionMode, setPermissionMode] =
    useState<PermissionMode>("default");
  const [resumeMode, setResumeMode] = useState<ResumeMode>("new");
  const [modelMode, setModelMode] = useState<ModelMode>("provider_default");
  const [launchMode, setLaunchMode] = useState<LaunchMode>("auto");
  const [providerMode, setProviderMode] =
    useState<ProviderMode>("selected_provider");
  const [snapshot, setSnapshot] = useState<ProviderRuntimeSnapshot | null>(
    null,
  );
  const [snapshotError, setSnapshotError] = useState<string | null>(null);
  const [sessionPickerOpen, setSessionPickerOpen] = useState(false);
  const [sessionsLoading, setSessionsLoading] = useState(false);
  const [sessions, setSessions] = useState<SessionMeta[]>([]);
  const [form, setForm] = useState({
    name: lang === "zh" ? "Claude 智能体" : "Claude Agent",
    runtime: "claude_code" as AgentRuntimeKind,
    providerId: "",
    model: "",
    runProfileId: "safe",
    cwd: "",
    sessionId: "",
  });
  const selectedProvider = useMemo(
    () => providers.find((provider) => provider.id === form.providerId),
    [form.providerId, providers],
  );
  const providerModelOptions = useMemo(
    () => extractProviderModels(selectedProvider?.settingsConfig),
    [selectedProvider?.settingsConfig],
  );

  const getAgentProviderName = useCallback(
    (agent: AgentInstance) =>
      agent.providerName ||
      providerNameById.get(agent.providerId) ||
      shortId(agent.providerId),
    [providerNameById],
  );

  const copyText = useCallback(
    async (label: string, value?: string | null) => {
      if (!value) return;
      await navigator.clipboard.writeText(value);
      toast.success(`${tt.copy}: ${label}`);
    },
    [tt.copy],
  );

  const availableProfiles =
    runProfiles.length > 0 ? runProfiles : builtInProfiles(lang);
  const selectedPermission = permissionOptions.find(
    (item) => item.value === permissionMode,
  );
  const commandPreview = useMemo(() => {
    const parts = ["claude"];
    const effectiveModel = modelMode === "custom" ? form.model.trim() : "";
    if (effectiveModel && isClaudeCodeEntryModel(effectiveModel)) {
      parts.push("--model", effectiveModel);
    }
    if (resumeMode === "resume_id") {
      parts.push("--resume", form.sessionId.trim() || "<会话ID>");
    }
    if (selectedPermission?.commandArg) {
      parts.push(...selectedPermission.commandArg.split(" "));
    }
    return parts.join(" ");
  }, [form.model, form.sessionId, modelMode, resumeMode, selectedPermission]);

  useEffect(() => {
    if (!form.providerId && providers.length > 0) {
      setForm((current) => ({
        ...current,
        providerId: providersData?.currentProviderId || providers[0].id,
      }));
    }
  }, [form.providerId, providers, providersData?.currentProviderId]);

  useEffect(() => {
    let cancelled = false;
    const loadSnapshot = async () => {
      const providerId =
        providerMode === "current_cc_switch_provider"
          ? providersData?.currentProviderId || form.providerId
          : form.providerId;
      if (!providerId) {
        setSnapshot(null);
        setSnapshotError(null);
        return;
      }
      try {
        const next = await agentGatewayApi.previewProviderSnapshot({
          providerId,
          providerMode,
        });
        if (!cancelled) {
          setSnapshot(next);
          setSnapshotError(null);
        }
      } catch (error) {
        if (!cancelled) {
          setSnapshot(null);
          setSnapshotError(formatAgentError(error));
        }
      }
    };
    void loadSnapshot();
    return () => {
      cancelled = true;
    };
  }, [form.providerId, providerMode, providersData?.currentProviderId]);

  useEffect(() => {
    setForm((current) => ({
      ...current,
      runProfileId: resumeMode === "resume_id" ? "resume" : "safe",
    }));
  }, [resumeMode]);

  useEffect(() => {
    if (modelMode !== "provider_default") return;
    setForm((current) => ({
      ...current,
      model: "",
    }));
  }, [modelMode, providerModelOptions]);

  const selectedUpstreamModel = providerModelOptions[0] ?? "";
  const snapshotUpstreamModel =
    snapshot?.defaultUpstreamModel ||
    snapshot?.upstreamModels?.[0] ||
    selectedUpstreamModel;
  const providerMissingReason = useMemo(() => {
    if (!snapshot) return snapshotError;
    if (!snapshot.redactedBaseUrl) return tt.baseUrlMissing;
    if (!snapshot.authTokenPresent) return tt.apiKeyMissing;
    return null;
  }, [snapshot, snapshotError, tt.apiKeyMissing, tt.baseUrlMissing]);

  const changeLanguage = async (next: UiLang) => {
    setLang(next);
    localStorage.setItem("language", next);
    await i18n.changeLanguage(next);
  };

  const showError = (error: unknown) => {
    const message = formatAgentError(error)
      .replace("DB_MIGRATION_FAILED:", "数据库操作失败：")
      .replace("Details:", "详细信息：");
    setLastError(message);
    toast.error(message);
  };

  const refresh = useCallback(async () => {
    setLoading(true);
    setLastError(null);
    try {
      const [nextAgents, nextProfiles] = await Promise.all([
        agentGatewayApi.syncStatus(),
        agentGatewayApi.listRunProfiles(),
      ]);
      setAgents(nextAgents);
      setRunProfiles(nextProfiles);
      if (!selectedAgentId && nextAgents.length > 0) {
        setSelectedAgentId(nextAgents[0].id);
      }
    } catch (error) {
      showError(error);
    } finally {
      setLoading(false);
    }
  }, [selectedAgentId]);

  const refreshLogs = useCallback(async (agentId: string) => {
    if (!agentId) {
      setLogs([]);
      return;
    }
    try {
      setLogs(await agentGatewayApi.getLogs(agentId, 100));
    } catch (error) {
      showError(error);
    }
  }, []);

  useEffect(() => {
    void refresh();
    // Auto-poll every 5s for real-time status updates
    const interval = setInterval(() => void refresh(), 5000);
    return () => clearInterval(interval);
  }, [refresh]);

  const runDiagnostics = useCallback(async () => {
    setDiagnosticsLoading(true);
    try {
      setDiagnosticsReport(await diagnosticsApi.runAll());
    } catch (error) {
      showError(error);
    } finally {
      setDiagnosticsLoading(false);
    }
  }, []);

  useEffect(() => {
    void runDiagnostics();
  }, [runDiagnostics]);

  useEffect(() => {
    void refreshLogs(selectedAgentId);
  }, [refreshLogs, selectedAgentId]);

  const selectedAgent = agents.find((agent) => agent.id === selectedAgentId);
  const selectedAgentProviderName = selectedAgent
    ? getAgentProviderName(selectedAgent)
    : "-";
  const canLaunch =
    form.runtime === "claude_code" &&
    Boolean(form.providerId) &&
    !loading &&
    !providerMissingReason &&
    (resumeMode === "new" || Boolean(form.sessionId.trim()));

  const launch = async () => {
    setLoading(true);
    setLastError(null);
    try {
      const agent = await agentGatewayApi.launchAgent({
        name:
          form.name.trim() ||
          (lang === "zh" ? "Claude 智能体" : "Claude Agent"),
        runtime: form.runtime,
        providerId:
          providerMode === "current_cc_switch_provider"
            ? providersData?.currentProviderId || form.providerId
            : form.providerId,
        providerMode,
        model:
          modelMode === "custom" && isClaudeCodeEntryModel(form.model)
            ? form.model.trim() || null
            : snapshotUpstreamModel || null,
        claudeEntryModel:
          modelMode === "custom" && isClaudeCodeEntryModel(form.model)
            ? form.model.trim() || null
            : null,
        upstreamProviderModel: snapshotUpstreamModel || null,
        runProfileId: resumeMode === "resume_id" ? "resume" : "safe",
        cwd: form.cwd.trim() || null,
        sessionId:
          resumeMode === "resume_id" ? form.sessionId.trim() || null : null,
        launchStrategy: launchMode === "auto" ? null : launchMode,
        permissionMode,
      });
      setSelectedAgentId(agent.id);
      toast.success(tt.successLaunch);
      // Reset form after successful launch
      setResumeMode("new");
      setModelMode("provider_default");
      setForm((current) => ({
        ...current,
        name: lang === "zh" ? "Claude 智能体" : "Claude Agent",
        model: "",
        sessionId: "",
        runProfileId: "safe",
      }));
      await refresh();
    } catch (error) {
      showError(error);
    } finally {
      setLoading(false);
    }
  };

  const stopOrKill = async (agent: AgentInstance, force: boolean) => {
    setActionId(agent.id);
    setLastError(null);
    try {
      if (force) {
        await agentGatewayApi.killAgent(agent.id);
        toast.success(tt.successKill);
      } else {
        await agentGatewayApi.stopAgent(agent.id);
        toast.success(tt.successStop);
      }
      await refresh();
      await refreshLogs(agent.id);
    } catch (error) {
      showError(error);
    } finally {
      setActionId(null);
    }
  };

  const resumeAgent = async (agent: AgentInstance) => {
    setActionId(agent.id);
    setLastError(null);
    try {
      const next = await agentGatewayApi.restartAgent(agent.id, {
        launchStrategy: launchMode === "auto" ? null : launchMode,
        permissionMode,
      });
      setSelectedAgentId(next.id);
      toast.success(tt.successRestart);
      await refresh();
    } catch (error) {
      showError(error);
    } finally {
      setActionId(null);
    }
  };

  const deleteAgent = async (agent: AgentInstance) => {
    if (!terminalStatuses.has(agent.status)) return;
    if (!window.confirm(tt.deleteConfirm)) return;
    setActionId(agent.id);
    setLastError(null);
    try {
      await agentGatewayApi.deleteAgent(agent.id);
      toast.success(tt.successDelete);
      if (selectedAgentId === agent.id) {
        setSelectedAgentId("");
        setLogs([]);
      }
      setSelectedAgentIds((prev) => {
        const next = new Set(prev);
        next.delete(agent.id);
        return next;
      });
      await refresh();
    } catch (error) {
      showError(error);
    } finally {
      setActionId(null);
    }
  };

  const deleteSelectedAgents = async () => {
    const deletableAgents = agents.filter(
      (agent) =>
        selectedAgentIds.has(agent.id) && terminalStatuses.has(agent.status),
    );
    if (deletableAgents.length === 0) return;
    if (
      !window.confirm(`${tt.deleteConfirm}\n\n${deletableAgents.length} agents`)
    )
      return;
    setLastError(null);
    try {
      await Promise.all(
        deletableAgents.map((agent) => agentGatewayApi.deleteAgent(agent.id)),
      );
      toast.success(`${deletableAgents.length} ${tt.successDelete}`);
      setSelectedAgentIds(new Set());
      if (deletableAgents.some((a) => a.id === selectedAgentId)) {
        setSelectedAgentId("");
        setLogs([]);
      }
      await refresh();
    } catch (error) {
      showError(error);
    }
  };

  const toggleAgentSelection = (agentId: string) => {
    setSelectedAgentIds((prev) => {
      const next = new Set(prev);
      if (next.has(agentId)) {
        next.delete(agentId);
      } else {
        next.add(agentId);
      }
      return next;
    });
  };

  const toggleSelectAll = () => {
    const deletableAgents = agents.filter((agent) =>
      terminalStatuses.has(agent.status),
    );
    if (selectedAgentIds.size === deletableAgents.length) {
      setSelectedAgentIds(new Set());
    } else {
      setSelectedAgentIds(new Set(deletableAgents.map((a) => a.id)));
    }
  };

  const exportDiagnostics = async () => {
    setDiagnosticsLoading(true);
    try {
      const path = await diagnosticsApi.exportReport();
      toast.success(`${tt.exportSuccess}: ${path}`);
    } catch (error) {
      showError(error);
    } finally {
      setDiagnosticsLoading(false);
    }
  };

  const chooseProjectDirectory = async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title:
          lang === "zh" ? "选择 Agent 项目目录" : "Choose Agent project folder",
      });
      if (typeof selected === "string") {
        setForm((current) => ({ ...current, cwd: selected }));
      }
    } catch (error) {
      showError(error);
    }
  };

  const openSessionPicker = async () => {
    setSessionPickerOpen(true);
    setSessionsLoading(true);
    try {
      const all = await sessionsApi.list();
      setSessions(all.filter((session) => session.providerId === "claude"));
    } catch (error) {
      showError(error);
    } finally {
      setSessionsLoading(false);
    }
  };

  const chooseSession = (session: SessionMeta) => {
    setResumeMode("resume_id");
    setForm((current) => ({
      ...current,
      sessionId: session.sessionId,
      cwd: session.projectDir || current.cwd,
      runProfileId: "resume",
    }));
    setSessionPickerOpen(false);
    toast.success(
      lang === "zh"
        ? `将恢复 Claude Code 会话：${shortId(session.sessionId)}`
        : `Will resume Claude Code session: ${shortId(session.sessionId)}`,
    );
  };

  return (
    <div className="px-6 pb-6 flex flex-col flex-1 min-h-0 gap-4 overflow-auto">
      <div className="flex flex-col gap-3">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <div className="flex items-center gap-2">
              <Bot className="w-5 h-5 text-primary" />
              <h2 className="text-2xl font-semibold">{tt.title}</h2>
              <Badge variant="outline">{tt.native}</Badge>
            </div>
            <p className="text-sm text-muted-foreground mt-1 max-w-3xl">
              {tt.intro}
            </p>
          </div>
          <div className="flex gap-2">
            <Select
              value={lang}
              onValueChange={(value) => void changeLanguage(value as UiLang)}
            >
              <SelectTrigger className="w-[120px]">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="zh">中文</SelectItem>
                <SelectItem value="en">English</SelectItem>
              </SelectContent>
            </Select>
            <Button
              variant="outline"
              size="sm"
              onClick={refresh}
              disabled={loading}
            >
              <RefreshCw className={cn("w-4 h-4", loading && "animate-spin")} />
              {tt.refresh}
            </Button>
          </div>
        </div>

        {lastError && (
          <div className="rounded-lg border border-amber-500/40 bg-amber-500/10 px-4 py-3 text-sm text-amber-900 flex gap-2">
            <AlertTriangle className="w-4 h-4 mt-0.5 flex-none" />
            <span>{lastError}</span>
          </div>
        )}
      </div>

      <section className="glass-card rounded-lg p-4">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <div className="flex items-center gap-2">
              <ShieldCheck className="w-4 h-4 text-primary" />
              <h3 className="text-base font-semibold">{tt.doctor}</h3>
              {diagnosticsReport && (
                <Badge
                  variant="outline"
                  className={
                    diagnosticsReport.summary.errors > 0
                      ? "border-red-500/40 bg-red-500/10 text-red-700"
                      : diagnosticsReport.summary.warnings > 0
                        ? "border-amber-500/40 bg-amber-500/10 text-amber-700"
                        : "border-emerald-500/40 bg-emerald-500/10 text-emerald-700"
                  }
                >
                  {diagnosticsReport.summary.errors} errors ·{" "}
                  {diagnosticsReport.summary.warnings} warnings
                </Badge>
              )}
            </div>
            <p className="text-xs text-muted-foreground mt-1">
              {tt.doctorHint}
            </p>
          </div>
          <div className="flex gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={runDiagnostics}
              disabled={diagnosticsLoading}
            >
              <RefreshCw
                className={cn("w-4 h-4", diagnosticsLoading && "animate-spin")}
              />
              {tt.run}
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={exportDiagnostics}
              disabled={diagnosticsLoading}
            >
              <FileText className="w-4 h-4" />
              {tt.export}
            </Button>
          </div>
        </div>
      </section>

      <div className="grid grid-cols-1 xl:grid-cols-[420px_minmax(0,1fr)] gap-4 min-h-[720px]">
        <section className="glass-card rounded-lg p-4 flex flex-col gap-4">
          <div className="flex items-center gap-2">
            <Terminal className="w-4 h-4 text-primary" />
            <h3 className="text-base font-semibold">{tt.launchTitle}</h3>
          </div>

          <div className="grid gap-3">
            <div className="grid gap-1.5">
              <Label htmlFor="agent-name">{tt.name}</Label>
              <Input
                id="agent-name"
                value={form.name}
                onChange={(event) =>
                  setForm((current) => ({
                    ...current,
                    name: event.target.value,
                  }))
                }
              />
            </div>

            <div className="grid grid-cols-2 gap-3">
              <div className="grid gap-1.5">
                <Label>{tt.runtime}</Label>
                <Select
                  value={form.runtime}
                  onValueChange={(value) =>
                    setForm((current) => ({
                      ...current,
                      runtime: value as AgentRuntimeKind,
                    }))
                  }
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {RUNTIME_OPTIONS.map((runtime) => (
                      <SelectItem
                        key={runtime.value}
                        value={runtime.value}
                        disabled={runtime.disabled}
                      >
                        {runtime[lang]}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>

              <div className="grid gap-1.5">
                <Label>{tt.permission}</Label>
                <Select
                  value={permissionMode}
                  onValueChange={(value) =>
                    setPermissionMode(value as PermissionMode)
                  }
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {permissionOptions.map((item) => (
                      <SelectItem key={item.value} value={item.value}>
                        {item[lang]}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </div>

            <div className="rounded-lg border border-border-default bg-white/[0.03] p-3 text-xs text-muted-foreground">
              {selectedPermission?.value === "default" && tt.safeDesc}
              {selectedPermission?.value === "plan" && tt.planDesc}
              {selectedPermission?.value === "accept_edits" &&
                tt.acceptEditsDesc}
              {selectedPermission?.value === "auto" && tt.autoDesc}
              {selectedPermission?.value === "dont_ask" && tt.dontAskDesc}
              {selectedPermission?.value === "bypass_permissions" &&
                tt.dangerDesc}
            </div>

            <div className="rounded-lg border border-blue-500/30 bg-blue-500/10 p-3 text-xs">
              <div className="font-medium text-foreground mb-1">
                {tt.commandPreview}
              </div>
              <code className="block rounded-md bg-background/80 px-3 py-2 font-mono text-[12px] break-all">
                {commandPreview}
              </code>
              <p className="mt-2 text-muted-foreground">
                {tt.commandPreviewHint}
              </p>
              {selectedUpstreamModel ? (
                <div className="mt-3 rounded-md bg-background/70 px-3 py-2">
                  <div className="font-medium text-foreground">
                    {tt.upstreamModel}
                  </div>
                  <code className="block font-mono break-all">
                    {selectedUpstreamModel}
                  </code>
                  <p className="mt-1 text-muted-foreground">
                    {tt.upstreamModelHint}
                  </p>
                </div>
              ) : null}
              {modelMode === "custom" &&
              form.model.trim() &&
              !isClaudeCodeEntryModel(form.model) ? (
                <p className="mt-2 text-amber-700">{tt.entryModelHint}</p>
              ) : null}
              <div className="mt-3 space-y-1 text-muted-foreground">
                <div className="font-medium text-foreground">
                  {lang === "zh" ? "常用命令格式" : "Common command formats"}
                </div>
                <code className="block font-mono">claude</code>
                <code className="block font-mono">
                  claude --dangerously-skip-permissions
                </code>
                <code className="block font-mono">
                  claude --resume &lt;{lang === "zh" ? "会话ID" : "session_id"}
                  &gt; --dangerously-skip-permissions
                </code>
                <code className="block font-mono">
                  claude --permission-mode
                  default|acceptEdits|plan|auto|dontAsk|bypassPermissions
                </code>
              </div>
            </div>

            <div className="grid gap-1.5">
              <Label>{tt.launchMode}</Label>
              <Select
                value={launchMode}
                onValueChange={(value) => setLaunchMode(value as LaunchMode)}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="auto">{tt.launchAuto}</SelectItem>
                  <SelectItem value="windows_terminal">
                    {tt.launchWt}
                  </SelectItem>
                  <SelectItem value="power_shell_window">
                    {tt.launchPs}
                  </SelectItem>
                  <SelectItem value="background_process">
                    {tt.launchBg}
                  </SelectItem>
                </SelectContent>
              </Select>
              <p className="text-xs text-muted-foreground">{tt.launchHint}</p>
            </div>

            <div className="grid gap-1.5">
              <Label>{tt.providerMode}</Label>
              <Select
                value={providerMode}
                onValueChange={(value) =>
                  setProviderMode(value as ProviderMode)
                }
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="selected_provider">
                    {tt.selectedProviderMode}
                  </SelectItem>
                  <SelectItem value="current_cc_switch_provider">
                    {tt.currentProviderMode}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>

            <div className="grid gap-1.5">
              <Label>{tt.provider}</Label>
              <Select
                value={form.providerId}
                onValueChange={(value) =>
                  setForm((current) => ({ ...current, providerId: value }))
                }
                disabled={
                  providersLoading ||
                  providers.length === 0 ||
                  providerMode === "current_cc_switch_provider"
                }
              >
                <SelectTrigger>
                  <SelectValue
                    placeholder={globalT("provider.select", {
                      defaultValue: "选择供应商",
                    })}
                  />
                </SelectTrigger>
                <SelectContent>
                  {providers.map((provider) => (
                    <SelectItem key={provider.id} value={provider.id}>
                      {provider.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div
              className={cn(
                "rounded-lg border p-3 text-xs space-y-2",
                providerMissingReason
                  ? "border-amber-500/40 bg-amber-500/10"
                  : "border-emerald-500/30 bg-emerald-500/10",
              )}
            >
              <div className="flex items-center justify-between gap-2">
                <div className="font-medium text-foreground">
                  {tt.snapshotTitle}
                </div>
                <Badge variant="outline">
                  {snapshot?.authTokenPresent
                    ? tt.apiKeyConfigured
                    : tt.apiKeyMissing}
                </Badge>
              </div>
              {snapshot ? (
                <div className="grid grid-cols-1 gap-1 text-muted-foreground">
                  <div>
                    {snapshot.providerName} · {shortId(snapshot.providerId)}
                  </div>
                  <div>
                    Type: {snapshot.providerType || "-"} · API:{" "}
                    {snapshot.apiFormat || "-"}
                  </div>
                  <div>Base URL: {snapshot.redactedBaseUrl || "-"}</div>
                  <div>
                    {tt.upstreamModel}: {snapshotUpstreamModel || "-"}
                  </div>
                  <div>{tt.sourceDb}</div>
                  <div>{tt.nativeModeLine}</div>
                </div>
              ) : (
                <div className="text-muted-foreground">
                  {snapshotError || "-"}
                </div>
              )}
              <p className="text-muted-foreground">{tt.snapshotHint}</p>
              {providerMissingReason ? (
                <p className="text-amber-800">{providerMissingReason}</p>
              ) : null}
            </div>

            <div className="grid grid-cols-2 gap-3">
              <div className="grid gap-1.5">
                <Label>{tt.modelMode}</Label>
                <Select
                  value={modelMode}
                  onValueChange={(value) => setModelMode(value as ModelMode)}
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="provider_default">
                      {tt.providerDefault}
                    </SelectItem>
                    <SelectItem value="custom">{tt.customModel}</SelectItem>
                  </SelectContent>
                </Select>
              </div>
              <div className="grid gap-1.5">
                <Label htmlFor="agent-model">{tt.model}</Label>
                {modelMode === "provider_default" &&
                providerModelOptions.length > 0 ? (
                  <Select
                    value={providerModelOptions[0]}
                    onValueChange={() => undefined}
                    disabled
                  >
                    <SelectTrigger id="agent-model">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {providerModelOptions.map((model) => (
                        <SelectItem key={model} value={model}>
                          {model}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                ) : (
                  <Input
                    id="agent-model"
                    disabled={modelMode !== "custom"}
                    placeholder={tt.optional}
                    value={form.model}
                    onChange={(event) =>
                      setForm((current) => ({
                        ...current,
                        model: event.target.value,
                      }))
                    }
                  />
                )}
              </div>
            </div>

            <div className="grid grid-cols-2 gap-3">
              <div className="grid gap-1.5">
                <Label>{tt.resumeMode}</Label>
                <Select
                  value={resumeMode}
                  onValueChange={(value) => setResumeMode(value as ResumeMode)}
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="new">{tt.newSession}</SelectItem>
                    <SelectItem value="resume_id">{tt.resumeById}</SelectItem>
                  </SelectContent>
                </Select>
              </div>
              <div className="grid gap-1.5">
                <Label>{tt.runProfile}</Label>
                <Select value={form.runProfileId} disabled>
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {availableProfiles.map((profile) => (
                      <SelectItem key={profile.id} value={profile.id}>
                        {profile.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </div>

            <div className="grid gap-1.5">
              <Label htmlFor="agent-session">{tt.sessionId}</Label>
              <Input
                id="agent-session"
                disabled={resumeMode !== "resume_id"}
                placeholder={
                  resumeMode === "resume_id" ? tt.resumeById : tt.optional
                }
                value={form.sessionId}
                onChange={(event) =>
                  setForm((current) => ({
                    ...current,
                    sessionId: event.target.value,
                  }))
                }
              />
              <div className="flex items-center gap-2">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={resumeMode !== "resume_id"}
                  onClick={() => void openSessionPicker()}
                >
                  {tt.chooseSession}
                </Button>
                <span className="text-xs text-muted-foreground">
                  {tt.sessionHelp}
                </span>
              </div>
            </div>

            <div className="grid gap-1.5">
              <Label htmlFor="agent-cwd">{tt.projectPath}</Label>
              <div className="flex gap-2">
                <Input
                  id="agent-cwd"
                  placeholder={tt.optional}
                  value={form.cwd}
                  onChange={(event) =>
                    setForm((current) => ({
                      ...current,
                      cwd: event.target.value,
                    }))
                  }
                />
                <Button
                  type="button"
                  variant="outline"
                  className="shrink-0"
                  onClick={() => void chooseProjectDirectory()}
                >
                  <FolderOpen className="w-4 h-4" />
                  {tt.chooseDirectory}
                </Button>
              </div>
            </div>
          </div>

          <div className="rounded-lg border border-border-default bg-white/[0.03] p-3 text-xs text-muted-foreground space-y-2">
            <div className="flex items-center gap-2 text-foreground">
              <ShieldCheck className="w-4 h-4 text-emerald-500" />
              <span>{tt.dataSafety}</span>
            </div>
            <p>{tt.dataSafetyText}</p>
          </div>

          <div className="flex gap-2">
            <Button onClick={launch} disabled={!canLaunch} className="flex-1">
              <Play className="w-4 h-4" />
              {tt.launch}
            </Button>
            <Button
              variant="outline"
              onClick={() => {
                setResumeMode("new");
                setModelMode("provider_default");
                setForm((current) => ({
                  ...current,
                  name: lang === "zh" ? "Claude 智能体" : "Claude Agent",
                  model: "",
                  sessionId: "",
                  runProfileId: "safe",
                }));
              }}
            >
              {lang === "zh" ? "重置" : "Reset"}
            </Button>
          </div>
        </section>

        <section className="glass-card rounded-lg p-4 flex flex-col min-h-0">
          <div className="flex flex-wrap items-center justify-between gap-2 mb-3">
            <div>
              <h3 className="text-base font-semibold">{tt.monitor}</h3>
              <p className="text-xs text-muted-foreground">{tt.monitorHint}</p>
            </div>
            <div className="flex items-center gap-2">
              {selectedAgentIds.size > 0 && (
                <Button
                  variant="destructive"
                  size="sm"
                  onClick={() => void deleteSelectedAgents()}
                >
                  <Trash2 className="w-3.5 h-3.5 mr-1" />
                  {lang === "zh"
                    ? `删除 ${selectedAgentIds.size} 个`
                    : `Delete ${selectedAgentIds.size}`}
                </Button>
              )}
              <Badge variant="outline">{agents.length} agents</Badge>
              <Button
                variant={groupByFolder ? "default" : "outline"}
                size="sm"
                onClick={() => setGroupByFolder(!groupByFolder)}
                className="ml-1"
              >
                {lang === "zh" ? "按目录分组" : "Group by folder"}
              </Button>
            </div>
          </div>

          {groupByFolder ? (
            <div className="min-h-[260px] flex-1 overflow-auto border border-border-default rounded-lg">
              {(() => {
                const grouped = new Map<string, AgentInstance[]>();
                agents.forEach((a) => {
                  const folder =
                    a.cwd
                      ?.split(/[\/\\]/)
                      .filter(Boolean)
                      .pop() ?? "(no folder)";
                  if (!grouped.has(folder)) grouped.set(folder, []);
                  grouped.get(folder)!.push(a);
                });
                return Array.from(grouped.entries()).map(
                  ([folder, folderAgents]) => (
                    <details
                      key={folder}
                      className="border-b border-border-default last:border-b-0"
                      open
                    >
                      <summary className="sticky top-0 z-10 flex items-center gap-2 px-4 py-2 text-sm font-medium cursor-pointer bg-bgApp hover:bg-bgAppHover select-none">
                        <span>📁 {folder}</span>
                        <span className="text-xs text-muted-foreground">
                          ({folderAgents.length})
                        </span>
                      </summary>
                      <div className="px-4 py-1 space-y-1">
                        {folderAgents.map((agent) => (
                          <div
                            key={agent.id}
                            className={cn(
                              "flex items-center justify-between rounded-md px-3 py-2 cursor-pointer text-sm",
                              selectedAgentId === agent.id && "bg-white/[0.04]",
                            )}
                            onClick={() => setSelectedAgentId(agent.id)}
                          >
                            <div className="flex items-center gap-2 min-w-0">
                              <input
                                type="checkbox"
                                checked={selectedAgentIds.has(agent.id)}
                                disabled={!terminalStatuses.has(agent.status)}
                                onChange={() => toggleAgentSelection(agent.id)}
                                className="cursor-pointer shrink-0"
                                onClick={(e) => e.stopPropagation()}
                              />
                              <div className="min-w-0">
                                <div className="flex items-center gap-2">
                                  <span className="font-medium truncate">
                                    {agent.name}
                                  </span>
                                  <Badge variant="outline" className="shrink-0">
                                    {agent.launchMode === "resume" ? "↺" : "●"}
                                  </Badge>
                                </div>
                                <div className="text-xs text-muted-foreground truncate">
                                  {agent.windowTitle ?? agent.id.slice(0, 8)}
                                </div>
                              </div>
                            </div>
                            <div className="flex items-center gap-2 shrink-0">
                              <Badge
                                variant="outline"
                                className={statusClassName[agent.status] ?? ""}
                              >
                                {agent.status}
                              </Badge>
                              <span className="text-xs text-muted-foreground">
                                {agent.providerName}
                              </span>
                            </div>
                          </div>
                        ))}
                      </div>
                    </details>
                  ),
                );
              })()}
            </div>
          ) : (
            <div className="min-h-[260px] flex-1 overflow-auto border border-border-default rounded-lg">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead className="w-10">
                      <input
                        type="checkbox"
                        checked={
                          selectedAgentIds.size > 0 &&
                          selectedAgentIds.size ===
                            agents.filter((a) => terminalStatuses.has(a.status))
                              .length
                        }
                        onChange={toggleSelectAll}
                        className="cursor-pointer"
                      />
                    </TableHead>
                    <TableHead>{tt.name}</TableHead>
                    <TableHead>{tt.status}</TableHead>
                    <TableHead>{tt.provider}</TableHead>
                    <TableHead className="text-right">{tt.actions}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {agents.map((agent) => (
                    <TableRow
                      key={agent.id}
                      className={cn(
                        "cursor-pointer",
                        selectedAgentId === agent.id && "bg-white/[0.04]",
                      )}
                      onClick={() => setSelectedAgentId(agent.id)}
                    >
                      <TableCell onClick={(e) => e.stopPropagation()}>
                        <input
                          type="checkbox"
                          checked={selectedAgentIds.has(agent.id)}
                          disabled={!terminalStatuses.has(agent.status)}
                          onChange={() => toggleAgentSelection(agent.id)}
                          className="cursor-pointer"
                        />
                      </TableCell>
                      <TableCell>
                        <div className="flex items-center gap-2">
                          <span className="font-medium">{agent.name}</span>
                          <Badge variant="outline">
                            {agent.launchMode === "resume"
                              ? tt.resumeModeTag
                              : tt.newMode}
                          </Badge>
                        </div>
                        <div className="text-xs text-muted-foreground">
                          {agent.windowTitle ?? `CCSA:${agent.id}`}
                        </div>
                        {agent.launchMode === "resume" && agent.sessionId ? (
                          <div className="text-xs text-muted-foreground">
                            {tt.sessionId}: {shortId(agent.sessionId)}
                          </div>
                        ) : null}
                      </TableCell>
                      <TableCell>
                        <Badge
                          variant="outline"
                          className={statusClassName[agent.status] ?? ""}
                        >
                          {agent.status}
                        </Badge>
                      </TableCell>
                      <TableCell>
                        <div className="text-sm">
                          {getAgentProviderName(agent)}
                        </div>
                        <div className="text-xs text-muted-foreground">
                          {agent.model || "-"} · {tt.port}: {agent.port}
                        </div>
                      </TableCell>
                      <TableCell>
                        <div className="flex justify-end gap-2 flex-nowrap">
                          <Button
                            variant="ghost"
                            size="icon"
                            title={tt.logs}
                            onClick={(event) => {
                              event.stopPropagation();
                              setSelectedAgentId(agent.id);
                              void refreshLogs(agent.id);
                            }}
                          >
                            <FileText className="w-4 h-4" />
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon"
                            title={tt.resume}
                            disabled={actionId === agent.id}
                            onClick={(event) => {
                              event.stopPropagation();
                              void resumeAgent(agent);
                            }}
                          >
                            <RotateCcw className="w-4 h-4" />
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon"
                            title="Stop"
                            disabled={actionId === agent.id}
                            onClick={(event) => {
                              event.stopPropagation();
                              void stopOrKill(agent, false);
                            }}
                          >
                            <CircleStop className="w-4 h-4" />
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon"
                            title="Kill process tree"
                            disabled={actionId === agent.id}
                            onClick={(event) => {
                              event.stopPropagation();
                              void stopOrKill(agent, true);
                            }}
                          >
                            <Skull className="w-4 h-4" />
                          </Button>
                          <Button
                            variant={
                              terminalStatuses.has(agent.status)
                                ? "destructive"
                                : "outline"
                            }
                            size="sm"
                            className={
                              !terminalStatuses.has(agent.status)
                                ? "opacity-60"
                                : ""
                            }
                            disabled={
                              actionId === agent.id ||
                              !terminalStatuses.has(agent.status)
                            }
                            title={
                              !terminalStatuses.has(agent.status)
                                ? tt.deleteDisabled
                                : tt.delete
                            }
                            onClick={(event) => {
                              event.stopPropagation();
                              void deleteAgent(agent);
                            }}
                          >
                            <Trash2 className="w-3.5 h-3.5 mr-1" />
                            {tt.delete}
                          </Button>
                        </div>
                      </TableCell>
                    </TableRow>
                  ))}
                  {agents.length === 0 && (
                    <TableRow>
                      <TableCell
                        colSpan={5}
                        className="text-center text-muted-foreground py-12"
                      >
                        {tt.noAgents}
                      </TableCell>
                    </TableRow>
                  )}
                </TableBody>
              </Table>
            </div>
          )}

          <div className="mt-4 grid grid-cols-1 lg:grid-cols-[minmax(0,1fr)_360px] gap-4 min-h-[180px]">
            <div className="rounded-lg border border-border-default bg-white/[0.03] p-3 overflow-auto">
              <div className="flex items-center gap-2 mb-2">
                <FileText className="w-4 h-4 text-primary" />
                <h4 className="text-sm font-medium">{tt.logs}</h4>
              </div>
              <div className="space-y-2 text-xs font-mono">
                {logs.map((log) => (
                  <div key={log.id} className="text-muted-foreground">
                    <span className="text-foreground">{log.createdAt}</span>{" "}
                    <span>{log.level}</span> <span>{log.event}</span>
                    {log.message ? <span> - {log.message}</span> : null}
                  </div>
                ))}
                {logs.length === 0 && (
                  <div className="text-muted-foreground">
                    {selectedAgent ? tt.noLogs : tt.selectAgent}
                  </div>
                )}
              </div>
            </div>

            <div className="rounded-lg border border-border-default bg-white/[0.03] p-3 text-sm">
              <h4 className="font-medium mb-2">{tt.selected}</h4>
              {selectedAgent ? (
                <>
                  <dl className="space-y-2 text-xs">
                    <div className="flex justify-between gap-3">
                      <dt className="text-muted-foreground">{tt.runtime}</dt>
                      <dd>{selectedAgent.runtime}</dd>
                    </div>
                    <div className="flex justify-between gap-3">
                      <dt className="text-muted-foreground">
                        {tt.launchModeTag}
                      </dt>
                      <dd>
                        {selectedAgent.launchMode === "resume"
                          ? tt.resumeModeTag
                          : tt.newMode}
                      </dd>
                    </div>
                    <div className="flex justify-between gap-3">
                      <dt className="text-muted-foreground">{tt.provider}</dt>
                      <dd className="truncate text-right">
                        {selectedAgentProviderName}
                      </dd>
                    </div>
                    <div className="flex justify-between gap-3">
                      <dt className="text-muted-foreground">{tt.providerId}</dt>
                      <dd className="flex items-center gap-1 min-w-0">
                        <span className="truncate">
                          {shortId(selectedAgent.providerId)}
                        </span>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-6 w-6 shrink-0"
                          onClick={() =>
                            void copyText(
                              tt.providerId,
                              selectedAgent.providerId,
                            )
                          }
                        >
                          <CopyIcon className="w-3 h-3" />
                        </Button>
                      </dd>
                    </div>
                    <div className="flex justify-between gap-3">
                      <dt className="text-muted-foreground">{tt.model}</dt>
                      <dd className="truncate text-right">
                        {selectedAgent.model || "-"}
                      </dd>
                    </div>
                    <div className="flex justify-between gap-3">
                      <dt className="text-muted-foreground">
                        {tt.localProxyUrl}
                      </dt>
                      <dd className="flex items-center gap-1 min-w-0">
                        <span className="truncate">
                          http://127.0.0.1:{selectedAgent.port}
                        </span>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-6 w-6 shrink-0"
                          onClick={() =>
                            void copyText(
                              tt.localProxyUrl,
                              `http://127.0.0.1:${selectedAgent.port}`,
                            )
                          }
                        >
                          <CopyIcon className="w-3 h-3" />
                        </Button>
                      </dd>
                    </div>
                    <div className="flex justify-between gap-3">
                      <dt className="text-muted-foreground">{tt.cwd}</dt>
                      <dd className="truncate text-right">
                        {selectedAgent.cwd || "-"}
                      </dd>
                    </div>
                    <div className="flex justify-between gap-3">
                      <dt className="text-muted-foreground">{tt.agentId}</dt>
                      <dd className="flex items-center gap-1 min-w-0">
                        <span className="truncate">
                          {shortId(selectedAgent.id)}
                        </span>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-6 w-6 shrink-0"
                          onClick={() =>
                            void copyText(tt.agentId, selectedAgent.id)
                          }
                        >
                          <CopyIcon className="w-3 h-3" />
                        </Button>
                      </dd>
                    </div>
                    <div className="flex justify-between gap-3">
                      <dt className="text-muted-foreground">{tt.ccsaId}</dt>
                      <dd className="flex items-center gap-1 min-w-0">
                        <span className="truncate">
                          {selectedAgent.windowTitle ??
                            `CCSA:${selectedAgent.id}`}
                        </span>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-6 w-6 shrink-0"
                          onClick={() =>
                            void copyText(
                              tt.ccsaId,
                              selectedAgent.windowTitle ??
                                `CCSA:${selectedAgent.id}`,
                            )
                          }
                        >
                          <CopyIcon className="w-3 h-3" />
                        </Button>
                      </dd>
                    </div>
                    {selectedAgent.sessionId ? (
                      <div className="flex justify-between gap-3">
                        <dt className="text-muted-foreground">
                          {tt.sessionId}
                        </dt>
                        <dd className="flex items-center gap-1 min-w-0">
                          <span className="truncate">
                            {shortId(selectedAgent.sessionId)}
                          </span>
                          <Button
                            variant="ghost"
                            size="icon"
                            className="h-6 w-6 shrink-0"
                            onClick={() =>
                              void copyText(
                                tt.sessionId,
                                selectedAgent.sessionId,
                              )
                            }
                          >
                            <CopyIcon className="w-3 h-3" />
                          </Button>
                        </dd>
                      </div>
                    ) : null}
                    <div className="flex justify-between gap-3">
                      <dt className="text-muted-foreground">{tt.startedAt}</dt>
                      <dd className="truncate text-right">
                        {formatTime(selectedAgent.startedAt)}
                      </dd>
                    </div>
                    <div className="flex justify-between gap-3">
                      <dt className="text-muted-foreground">{tt.stoppedAt}</dt>
                      <dd className="truncate text-right">
                        {formatTime(selectedAgent.stoppedAt)}
                      </dd>
                    </div>
                    <div className="flex justify-between gap-3">
                      <dt className="text-muted-foreground">{tt.duration}</dt>
                      <dd className="truncate text-right">
                        {formatDuration(
                          selectedAgent.startedAt,
                          selectedAgent.stoppedAt,
                        )}
                      </dd>
                    </div>
                    <div className="flex justify-between gap-3">
                      <dt className="text-muted-foreground">{tt.lastError}</dt>
                      <dd className="truncate text-right">
                        {selectedAgent.lastError || "-"}
                      </dd>
                    </div>
                  </dl>
                  <div className="mt-3 pt-3 border-t border-border-default">
                    <Button
                      variant={
                        terminalStatuses.has(selectedAgent.status)
                          ? "destructive"
                          : "outline"
                      }
                      size="sm"
                      className="w-full"
                      disabled={
                        actionId === selectedAgent.id ||
                        !terminalStatuses.has(selectedAgent.status)
                      }
                      title={
                        !terminalStatuses.has(selectedAgent.status)
                          ? tt.deleteDisabled
                          : ""
                      }
                      onClick={() => void deleteAgent(selectedAgent)}
                    >
                      <Trash2 className="w-3.5 h-3.5 mr-1" />
                      {tt.delete}
                    </Button>
                  </div>
                </>
              ) : (
                <p className="text-xs text-muted-foreground">
                  {lang === "zh"
                    ? "选择 Agent 后显示运行详情。"
                    : "Runtime details appear after an agent is selected."}
                </p>
              )}
            </div>
          </div>
        </section>
      </div>
      <Dialog open={sessionPickerOpen} onOpenChange={setSessionPickerOpen}>
        <DialogContent className="max-w-2xl max-h-[80vh] overflow-auto">
          <DialogHeader>
            <DialogTitle>{tt.sessionPickerTitle}</DialogTitle>
          </DialogHeader>
          {sessionsLoading ? (
            <div className="py-8 text-center text-muted-foreground">
              {tt.refresh}...
            </div>
          ) : sessions.length === 0 ? (
            <div className="py-8 text-center text-muted-foreground">
              {tt.noClaudeSessions}
            </div>
          ) : (
            <div className="space-y-2">
              {sessions.map((session) => (
                <div
                  key={session.sessionId}
                  className="flex items-start justify-between gap-3 rounded-lg border border-border-default p-3 hover:bg-white/[0.04] cursor-pointer"
                  onClick={() => chooseSession(session)}
                >
                  <div className="min-w-0 flex-1">
                    <div className="font-medium text-sm truncate">
                      {session.title || shortId(session.sessionId)}
                    </div>
                    <div className="text-xs text-muted-foreground mt-1 space-y-0.5">
                      <div>Session ID: {shortId(session.sessionId)}</div>
                      {session.projectDir && (
                        <div>Project: {session.projectDir}</div>
                      )}
                      {session.lastActiveAt && (
                        <div>
                          {tt.lastActiveAt}: {formatTime(session.lastActiveAt)}
                        </div>
                      )}
                      {session.createdAt && (
                        <div>
                          {tt.createdAt}: {formatTime(session.createdAt)}
                        </div>
                      )}
                      {session.resumeCommand && (
                        <code className="block font-mono text-[11px] mt-1">
                          {session.resumeCommand}
                        </code>
                      )}
                    </div>
                  </div>
                  <div className="flex gap-1 shrink-0">
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={(event) => {
                        event.stopPropagation();
                        void copyText(tt.sessionId, session.sessionId);
                      }}
                    >
                      <CopyIcon className="w-3.5 h-3.5 mr-1" />
                      {tt.copy}
                    </Button>
                    <Button
                      variant="default"
                      size="sm"
                      onClick={(event) => {
                        event.stopPropagation();
                        chooseSession(session);
                      }}
                    >
                      {tt.select}
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
}
