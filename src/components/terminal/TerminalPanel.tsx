import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import { DndContext, closestCenter, type DragEndEvent } from "@dnd-kit/core";
import {
  SortableContext,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import {
  TerminalSquare,
  Square,
  Trash2,
  Plus,
  FolderKanban,
  Loader2,
  RotateCcw,
  History,
  PanelLeftClose,
  PanelLeftOpen,
  PanelRightClose,
  PanelRightOpen,
  Gauge,
  Play,
  Maximize2,
  Pencil,
  ChevronsDown,
  ChevronsUp,
  ChevronRight,
  GitBranch,
} from "lucide-react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { APP_ICON_MAP, getAppLabel } from "@/config/appConfig";
import { settingsApi, terminalApi, type AppId } from "@/lib/api";
import type { ModelRateInfo } from "@/lib/api/terminal";
import { projectApi, type DevProject } from "@/lib/api/project";
import {
  useTerminalHub,
  type TerminalAliveStatus,
} from "@/hooks/useTerminalHub";
import { NewTerminalDialog } from "./NewTerminalDialog";
import { NewProjectDialog } from "./NewProjectDialog";
import { FileEditorDialog } from "./FileEditorDialog";
import { projectFilesApi, type GitStatusResult } from "@/lib/api/projectFiles";
import {
  type TerminalInstance,
  type TerminalTool,
  type ToolSessionInfo,
} from "@/types/terminal";
import { cn } from "@/lib/utils";

/** 内嵌终端（xterm.js）懒加载：避免测试环境与首屏加载 xterm */
const EmbeddedTerminalLazy = lazy(() =>
  import("./EmbeddedTerminal").then((module) => ({
    default: module.EmbeddedTerminal,
  })),
);

/** 支持「最近会话」恢复的工具（启动参数里注入 resume/session 标记）。 */
const SESSION_TOOLS: TerminalTool[] = ["claude", "opencode"];

/** 终端列表条目（树形子节点，支持拖拽排序）。 */
function SortableTerminalItem({
  instance,
  status,
  isSelected,
  isContextTarget,
  onSelect,
  onContextMenu,
}: {
  instance: TerminalInstance;
  status: TerminalAliveStatus | "stopped";
  isSelected: boolean;
  isContextTarget: boolean;
  onSelect: (instance: TerminalInstance) => void;
  onContextMenu: (event: React.MouseEvent, instance: TerminalInstance) => void;
}) {
  const { t } = useTranslation();
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: instance.id });

  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.55 : undefined,
  };

  return (
    <div
      ref={setNodeRef}
      style={style}
      onClick={() => onSelect(instance)}
      onContextMenu={(event) => onContextMenu(event, instance)}
      className={cn(
        "group flex h-7 w-full cursor-pointer items-center gap-1.5 rounded-md px-1.5 transition-colors",
        isSelected
          ? "bg-primary/10 font-medium text-primary"
          : "text-foreground/80 hover:bg-muted/40",
        isContextTarget && "bg-muted/60",
        isDragging && "z-10 bg-background shadow-lg",
      )}
      {...attributes}
      {...listeners}
    >
      {/* 应用 logo + 运行状态角标 */}
      <div className="relative shrink-0">
        <div className="flex h-5 w-5 items-center justify-center rounded border border-border/60 bg-background/80">
          {APP_ICON_MAP[instance.app as AppId]?.icon ?? (
            <TerminalSquare className="h-3 w-3 text-muted-foreground" />
          )}
        </div>
        <span
          className={cn(
            "absolute -bottom-0.5 -right-0.5 h-2 w-2 rounded-full ring-2 ring-background",
            status === "running"
              ? "bg-emerald-500"
              : status === "idle"
                ? "bg-amber-400"
                : "bg-muted-foreground/40",
          )}
        />
      </div>
      <span className="min-w-0 flex-1 truncate text-xs">{instance.name}</span>
      <span
        className={cn(
          "shrink-0 text-[9px]",
          status === "running"
            ? "text-emerald-600"
            : status === "idle"
              ? "text-amber-500"
              : "text-muted-foreground/60",
        )}
      >
        {status === "running"
          ? t("terminalHub.running", { defaultValue: "运行中" })
          : status === "idle"
            ? t("terminalHub.idle", { defaultValue: "空闲" })
            : t("terminalHub.stopped", { defaultValue: "已停止" })}
      </span>
    </div>
  );
}

/** 工具 → 会话恢复参数名。 */
const SESSION_ARG_FLAG: Partial<Record<TerminalTool, string>> = {
  claude: "--resume",
  opencode: "--session",
};

/** 需要从参数里剔除的旧会话标记（避免与新的 --resume/--session 冲突）。 */
const SESSION_ARG_TOKENS = new Set([
  "--resume",
  "--session",
  "--continue",
  "-c",
  "-r",
  "-s",
]);

/** 在参数串里替换/追加会话标记。 */
function mergeSessionArg(
  current: string,
  tool: TerminalTool,
  sessionId: string,
): string {
  const flag = SESSION_ARG_FLAG[tool] ?? "--resume";
  const tokens = current.split(/\s+/).filter(Boolean);
  const next: string[] = [];
  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i];
    if (SESSION_ARG_TOKENS.has(token)) {
      // 跳过标记及其值
      i += 1;
      continue;
    }
    next.push(token);
  }
  next.push(flag, sessionId);
  return next.join(" ");
}

/** 会话时间展示：MM-DD HH:mm */
function formatSessionTime(iso?: string): string {
  if (!iso) return "";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "";
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

/** token 数量展示：12345 → 12.3k，1234567 → 1.2M */
function formatTokenCount(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return String(n);
}

/** 请求耗时展示：12500 → 12.5s，125000 → 2m05s */
function formatDuration(ms: number): string {
  const safe = Math.max(0, ms);
  const secs = safe / 1000;
  if (secs < 60) return `${secs.toFixed(secs < 10 ? 1 : 0)}s`;
  const minutes = Math.floor(secs / 60);
  return `${minutes}m${String(Math.round(secs % 60)).padStart(2, "0")}s`;
}

/** 速率展示：<100 保留 1 位小数，≥100 取整，避免高位数字过宽导致抖动 */
function formatSpeed(v: number): string {
  return v >= 100 ? v.toFixed(0) : v.toFixed(1);
}

/** 模型名展示：去掉日期/快照后缀（如 -20250101），太长时从中间截断 */
function formatModelName(model: string): string {
  const trimmed = model.replace(/-\d{6,8}$/i, "");
  if (trimmed.length <= 24) return trimmed;
  return `${trimmed.slice(0, 14)}…${trimmed.slice(-8)}`;
}

/** git status 字母标记 → 状态色 */
function gitStatusColor(raw: string): string {
  if (raw.startsWith("??")) return "text-emerald-500";
  if (raw.includes("D")) return "text-red-500";
  if (raw.includes("A")) return "text-emerald-500";
  if (raw.includes("M")) return "text-amber-500";
  if (raw.includes("R")) return "text-sky-500";
  return "text-muted-foreground";
}

/** git status 两位码 → 可读说明（tooltip 用）。?? 表示未跟踪文件，不是文件名。 */
function gitStatusLabel(raw: string): string {
  const code = raw.trim();
  if (code === "??") return "未跟踪 (untracked)";
  if (code.includes("U") || code.includes("DD") || code.includes("AA"))
    return "冲突 (conflict)";
  if (code.includes("A")) return "已暂存新增 (added)";
  if (code.includes("R")) return "已重命名 (renamed)";
  if (code.includes("C")) return "已复制 (copied)";
  if (code.includes("M")) return "已修改 (modified)";
  if (code.includes("D")) return "已删除 (deleted)";
  return code || "变更 (changed)";
}

/** 实例存活状态：内嵌会话（后端 PTY）优先，其次系统终端 pid 轮询结果。 */
function instanceAliveStatus(
  instance: TerminalInstance,
  hub: ReturnType<typeof useTerminalHub>,
): TerminalAliveStatus | "stopped" {
  const embeddedOk =
    typeof hub.embeddedPty[instance.id] === "number" &&
    !hub.embeddedExit[instance.id];
  if (embeddedOk) return "running";
  if (typeof instance.pid === "number") {
    return hub.alive[instance.id] === "running" ? "running" : "stopped";
  }
  return "stopped";
}

export interface TerminalPanelProps {
  /** 当前选中的应用 id（决定新建终端时默认注入哪套环境变量） */
  activeApp: AppId;
  /** 各 AI 工具的启动参数模板（来自设置） */
  launchTemplates?: Partial<Record<TerminalTool, string>>;
  /** 右侧栏内容（模型/供应商列表），full 模式下渲染在终端最右侧 */
  rightPanel?: React.ReactNode;
  /** 紧凑模式（HUD 小窗）：左栏收窄，给终端留更多空间 */
  compact?: boolean;
  /** HUD 窗口模式：左栏标题栏作为窗口拖拽区 */
  hudDraggable?: boolean;
  /** HUD 拖拽区鼠标按下回调 */
  onHeaderDragStart?: (event: React.MouseEvent) => void;
  /** 仅选择模式（HUD 小窗）：项目点击只应用/选择，不展开文件树 */
  selectOnly?: boolean;
  /** HUD 模式：终端头部显示「还原大屏幕」按钮（点击恢复主窗口并隐藏 HUD） */
  onHudRestore?: () => void;
}

/** 左栏宽度（项目树 + 终端列表） */
const LEFT_WIDTH = 250;
/** 紧凑模式左栏宽度（HUD 小窗） */
const COMPACT_LEFT_WIDTH = 175;
/** 左栏折叠后宽度 */
const COLLAPSED_LEFT_WIDTH = 32;
/** 右侧栏宽度（模型列表） */
const RIGHT_WIDTH = 380;
/** 左栏折叠状态持久化 key */
const LEFT_COLLAPSED_KEY = "cc-switch-terminal-left-collapsed";
/** 右栏折叠状态持久化 key */
const RIGHT_COLLAPSED_KEY = "cc-switch-terminal-right-collapsed";
/** 选中终端持久化 key */
const SELECTED_STORAGE_KEY = "cc-switch-terminal-selected";
/** 选中项目持久化 key */
const PROJECT_STORAGE_KEY = "cc-switch-terminal-project";
/** 树形「未分组终端」虚拟分组 id */
const UNASSIGNED_GROUP_ID = "__unassigned__";

/**
 * 终端工作台（主页面三栏式布局）：
 * - 左栏：上半项目树 + 下半终端列表；
 * - 中栏：终端工作区（点击打开系统原生终端窗口）；
 * - 右栏：模型列表（供应商列表，由 rightPanel 注入）；
 * - 选中终端、终端实例均持久化保存。
 */
export function TerminalPanel({
  activeApp,
  launchTemplates,
  rightPanel,
  compact = false,
  hudDraggable = false,
  selectOnly = false,
  onHeaderDragStart,
  onHudRestore,
}: TerminalPanelProps) {
  const { t } = useTranslation();
  const hub = useTerminalHub();

  const [newOpen, setNewOpen] = useState(false);
  const [leftCollapsed, setLeftCollapsed] = useState(() => {
    try {
      return window.localStorage.getItem(LEFT_COLLAPSED_KEY) === "1";
    } catch {
      return false;
    }
  });
  const [confirmDelete, setConfirmDelete] = useState<TerminalInstance | null>(
    null,
  );
  /** 删除进行中的实例 id（确认对话框 pending 态，避免重复点击） */
  const [deletingId, setDeletingId] = useState<string | null>(null);
  /** 清除会话重新初始化进行中的实例 id（重置按钮 loading 态） */
  const [resettingId, setResettingId] = useState<string | null>(null);
  useEffect(() => {
    try {
      window.localStorage.setItem(
        LEFT_COLLAPSED_KEY,
        leftCollapsed ? "1" : "0",
      );
    } catch {
      // 忽略
    }
  }, [leftCollapsed]);
  const [rightCollapsed, setRightCollapsed] = useState(() => {
    try {
      return window.localStorage.getItem(RIGHT_COLLAPSED_KEY) === "1";
    } catch {
      return false;
    }
  });
  useEffect(() => {
    try {
      window.localStorage.setItem(
        RIGHT_COLLAPSED_KEY,
        rightCollapsed ? "1" : "0",
      );
    } catch {
      // 忽略
    }
  }, [rightCollapsed]);
  const [selectedId, setSelectedId] = useState<string | null>(() => {
    try {
      return window.localStorage.getItem(SELECTED_STORAGE_KEY);
    } catch {
      return null;
    }
  });
  /**
   * 已激活（保持挂载）的终端集合：选中即激活，此后常驻——切换标签只是
   * display:none 隐藏，xterm 屏幕状态原样保留。全屏 TUI 会话（Claude /
   * Codex 等）依靠 PTY 字节历史回放无法可靠还原屏幕（会切出空白），
   * 因此挂载过的终端不再卸载；应用重启后的恢复仍走后端历史回放。
   */
  const [activatedIds, setActivatedIds] = useState<Set<string>>(
    () => new Set(),
  );
  /** 删除进行中的实例 id（ref：effect 中用于抑制删除后的自动切换） */
  const deletingRef = useRef<string | null>(null);
  /** 开发项目：列表 + 当前选中（localStorage 记忆） */
  const [projects, setProjects] = useState<DevProject[]>([]);
  const [selectedProjectId, setSelectedProjectId] = useState<string | null>(
    () => {
      try {
        return window.localStorage.getItem(PROJECT_STORAGE_KEY);
      } catch {
        return null;
      }
    },
  );
  /** 当前项目的 Git 更改文件（底部区域展示） */
  const [gitStatus, setGitStatus] = useState<GitStatusResult | null>(null);
  const [projectDialogOpen, setProjectDialogOpen] = useState(false);
  const [projectBusy, setProjectBusy] = useState(false);
  const [confirmDeleteProject, setConfirmDeleteProject] =
    useState<DevProject | null>(null);
  /** 正在编辑的项目（null = 新建模式） */
  const [editingProject, setEditingProject] = useState<DevProject | null>(null);
  /** 「新建终端」目标项目 id（项目树行内 + 按钮） */
  const [newTerminalProjectId, setNewTerminalProjectId] = useState<
    string | null
  >(null);
  /** 树形节点展开集合（默认全部展开，新项目自动展开） */
  const [expandedIds, setExpandedIds] = useState<Set<string>>(() => new Set());
  /** 历史会话恢复下拉状态 */
  const [sessionsOpen, setSessionsOpen] = useState(false);
  const [sessionList, setSessionList] = useState<ToolSessionInfo[]>([]);
  const [sessionsLoading, setSessionsLoading] = useState(false);
  /** 当前调用模型的实时速率 */
  const [modelRate, setModelRate] = useState<ModelRateInfo | null>(null);
  /** 终端列表右键菜单 */
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    instance: TerminalInstance;
  } | null>(null);
  /** Git 更改点击 → 文件差异 / 编辑对话框 */
  const [editorFile, setEditorFile] = useState<{
    filePath: string;
    projectDir: string;
  } | null>(null);
  /** 项目右键菜单（编辑 / 删除） */
  const [projectContextMenu, setProjectContextMenu] = useState<{
    x: number;
    y: number;
    project: DevProject;
  } | null>(null);

  useEffect(() => {
    const close = () => {
      setContextMenu(null);
      setProjectContextMenu(null);
    };
    window.addEventListener("click", close);
    window.addEventListener("contextmenu", close);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("contextmenu", close);
      window.removeEventListener("blur", close);
    };
  }, []);

  // 实时速率轮询
  useEffect(() => {
    let cancelled = false;
    const tick = async () => {
      try {
        const rate = await terminalApi.getModelRate();
        if (cancelled) return;
        // 数值无变化时保持旧引用：避免每 2s 触发整个面板（含终端列表、
        // 项目树）无意义重渲染
        setModelRate((prev) =>
          prev &&
          prev.tokensPerSecond === rate.tokensPerSecond &&
          prev.outputTokens === rate.outputTokens &&
          prev.sampleSeconds === rate.sampleSeconds &&
          prev.totalTokens === rate.totalTokens &&
          prev.lastDurationMs === rate.lastDurationMs &&
          prev.lastModel === rate.lastModel &&
          prev.lastInputTokens === rate.lastInputTokens &&
          prev.lastOutputTokens === rate.lastOutputTokens &&
          prev.lastSpeedTokS === rate.lastSpeedTokS &&
          prev.sessionInputTokens === rate.sessionInputTokens &&
          prev.sessionOutputTokens === rate.sessionOutputTokens &&
          prev.sessionCacheTokens === rate.sessionCacheTokens &&
          prev.generating === rate.generating &&
          prev.liveSpeedTokS === rate.liveSpeedTokS
            ? prev
            : rate,
        );
      } catch {
        // 后端尚未就绪，忽略本轮
      }
    };
    void tick();
    const timer = window.setInterval(() => void tick(), 2000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, []);

  const selectedProject =
    projects.find((item) => item.id === selectedProjectId) ?? null;

  const effectiveApp: AppId =
    activeApp === "claude-desktop" ? "claude" : activeApp;
  const appLabel = getAppLabel(effectiveApp);

  const instances = hub.state.instances;
  /** 终端以项目为准：有选中项目时只显示该项目绑定的实例（兼容未绑定的旧实例） */
  const visibleInstances = selectedProject
    ? instances.filter(
        (item) => item.projectId === selectedProject.id || !item.projectId,
      )
    : instances;
  // 选中终端解析：selectedId 为空（如删除选中终端后）时显示空态，
  // 由用户点击列表项手动启动，避免删除瞬间自动弹出下一个终端会话。
  const selected = selectedId
    ? (instances.find((item) => item.id === selectedId) ?? null)
    : null;

  // 选中即激活：终端加入常驻挂载集合（切换标签只隐藏，不卸载）
  useEffect(() => {
    if (!selectedId) return;
    setActivatedIds((prev) =>
      prev.has(selectedId) ? prev : new Set(prev).add(selectedId),
    );
  }, [selectedId]);

  /** 常驻挂载的终端（过滤掉已删除的实例） */
  const activatedInstances = instances.filter((item) =>
    activatedIds.has(item.id),
  );

  // 选中终端持久化；列表变化时保证选中有效。
  // 注意：启动时 instances 先空后加载，空窗期不要清除已持久化的选中，等实例到位后再校验。
  useEffect(() => {
    if (instances.length === 0) return;
    if (!instances.some((item) => item.id === selectedId)) {
      // 用户主动删除当前选中的终端：不自动切换/启动下一个终端，
      // 由用户点击列表项手动启动（避免删除时弹出未预期的终端会话）
      if (deletingRef.current === selectedId) {
        deletingRef.current = null;
        setSelectedId(null);
        hub.setActiveInstanceId(null);
        try {
          window.localStorage.removeItem(SELECTED_STORAGE_KEY);
        } catch {
          // 忽略
        }
        return;
      }
      setSelectedId(instances[0].id);
      hub.setActiveInstanceId(instances[0].id);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [instances, selectedId]);

  // 把当前选中写入 localStorage
  useEffect(() => {
    if (selected) {
      try {
        window.localStorage.setItem(SELECTED_STORAGE_KEY, selected.id);
      } catch {
        // 忽略
      }
    }
  }, [selected]);

  // 当前选中终端是否处于活跃会话（系统终端 pid 存活 或 内嵌 PTY 未退出）
  const activeRunning = selected !== null && hub.instanceActive(selected.id);

  const handleBrowse = async (): Promise<string | null> => {
    try {
      const dir = await settingsApi.pickDirectory(hub.state.lastProjectDir);
      if (dir) hub.selectProjectDir(dir);
      return dir;
    } catch (error) {
      toast.error(
        t("terminalHub.browseFailed", { defaultValue: "选择目录失败" }) +
          `: ${String(error)}`,
      );
      return null;
    }
  };

  /** 点击列表项：切换右侧显示；内嵌终端会随选中自动启动/恢复会话。 */
  const selectInstance = (instance: TerminalInstance) => {
    setSelectedId(instance.id);
    hub.setActiveInstanceId(instance.id);
  };

  /** 拖拽排序终端列表（顺序持久化到后端）。 */
  const handleInstanceDragEnd = useCallback(
    (event: DragEndEvent) => {
      const { active, over } = event;
      if (!over || active.id === over.id) return;
      hub.reorderInstances(String(active.id), String(over.id));
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [hub.reorderInstances],
  );

  // 项目列表加载
  const loadProjects = useCallback(async () => {
    try {
      const list = await projectApi.list();
      setProjects(list);
      setSelectedProjectId((prev) => {
        const valid =
          prev && list.some((item) => item.id === prev) ? prev : null;
        try {
          window.localStorage.setItem(PROJECT_STORAGE_KEY, valid ?? "");
        } catch {
          // 忽略
        }
        return valid;
      });
    } catch (error) {
      console.error("[TerminalPanel] failed to load projects", error);
    }
  }, []);

  useEffect(() => {
    void loadProjects();
  }, [loadProjects]);

  /** 项目列表变化时同步展开集合：新项目默认展开，已删除的移除 */
  useEffect(() => {
    setExpandedIds((prev) => {
      const next = new Set(prev);
      let changed = false;
      const ids = new Set<string>();
      for (const project of projects) {
        ids.add(project.id);
        if (!next.has(project.id)) {
          next.add(project.id);
          changed = true;
        }
      }
      for (const id of prev) {
        if (!ids.has(id)) {
          next.delete(id);
          changed = true;
        }
      }
      return changed ? next : prev;
    });
  }, [projects]);

  const toggleExpanded = useCallback((id: string) => {
    setExpandedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  /** 拉取当前项目的 Git 更改文件（左栏底部区域展示）。 */
  const loadGitStatus = useCallback(async () => {
    const dir = selectedProject?.projectDir;
    if (!dir) {
      setGitStatus(null);
      return;
    }
    try {
      const result = await projectFilesApi.gitStatus(dir);
      // 内容无变化时保持旧引用，避免轮询触发无意义重渲染
      setGitStatus((prev) =>
        prev &&
        prev.git === result.git &&
        prev.entries.length === result.entries.length &&
        prev.entries.every(
          (entry, index) =>
            entry.path === result.entries[index]?.path &&
            entry.raw === result.entries[index]?.raw,
        )
          ? prev
          : result,
      );
    } catch {
      setGitStatus(null);
    }
  }, [selectedProject?.projectDir]);

  // Git 更改轮询：跟随选中项目切换，并持续刷新（编码过程中文件状态会变）。
  // 窗口失焦/隐藏时跳过轮询，聚焦时立即刷新一次——避免后台也每 8s 拉起
  // git 进程（浪费 CPU/IO，且曾是控制台窗口反复弹出的来源）。
  useEffect(() => {
    const active = () => !document.hidden && document.hasFocus();
    const poll = () => {
      if (!active()) return;
      void loadGitStatus();
    };
    poll();
    const handleFocus = () => {
      if (active()) void loadGitStatus();
    };
    window.addEventListener("focus", handleFocus);
    const timer = window.setInterval(poll, 8000);
    return () => {
      window.removeEventListener("focus", handleFocus);
      window.clearInterval(timer);
    };
  }, [loadGitStatus]);

  /** 选择项目即一键切换（应用供应商 + 恢复 Claude 配置快照 + 加载项目终端）。 */
  const handleApplyProject = async (id: string) => {
    const project = projects.find((item) => item.id === id);
    if (!project || projectBusy) return;
    setProjectBusy(true);
    // 应用项目时展开该项目的终端分组
    setExpandedIds((prev) => new Set(prev).add(project.id));
    try {
      const result = await projectApi.apply(id);
      setSelectedProjectId(id);
      try {
        window.localStorage.setItem(PROJECT_STORAGE_KEY, id);
      } catch {
        // 忽略
      }
      hub.selectProjectDir(project.projectDir);
      // 加载该项目的终端：优先项目绑定实例，其次通用（未绑定）实例
      const all = hub.state.instances;
      const bound = all.filter((item) => item.projectId === project.id);
      const generic = all.filter((item) => !item.projectId);
      const target = bound[0] ?? generic[0] ?? null;
      if (target) {
        setSelectedId(target.id);
        hub.setActiveInstanceId(target.id);
      }
      const toolsLabel = Object.keys(project.tools ?? {})
        .map(getAppLabel)
        .join("、");
      const snapshot = result.snapshot;
      const parts: string[] = [];
      if (snapshot.mcp) parts.push("MCP");
      if (snapshot.skills > 0) parts.push(`Skills×${snapshot.skills}`);
      if (snapshot.memory) parts.push("CLAUDE.md");
      toast.success(
        t("project.applied", {
          name: project.name,
          tools: toolsLabel,
          config: parts.length > 0 ? parts.join("、") : "无快照",
          defaultValue: `已应用项目「{{name}}」：{{tools}}；已恢复 {{config}}`,
        }),
        { closeButton: true, duration: 4000 },
      );
      if (result.warnings.length > 0) {
        toast.warning(result.warnings.join("；"), { closeButton: true });
      }
    } catch (error) {
      toast.error(
        t("project.applyFailed", {
          error: String(error),
          defaultValue: "应用项目失败：{{error}}",
        }),
        { closeButton: true },
      );
    } finally {
      setProjectBusy(false);
    }
  };

  const handleDeleteProject = async () => {
    if (!confirmDeleteProject) return;
    try {
      await projectApi.remove(confirmDeleteProject.id);
      setConfirmDeleteProject(null);
      if (selectedProjectId === confirmDeleteProject.id) {
        setSelectedProjectId(null);
        try {
          window.localStorage.setItem(PROJECT_STORAGE_KEY, "");
        } catch {
          // 忽略
        }
      }
      await loadProjects();
      toast.success(
        t("project.deleted", {
          name: confirmDeleteProject.name,
          defaultValue: `项目「${confirmDeleteProject.name}」已删除`,
        }),
        { closeButton: true },
      );
    } catch (error) {
      toast.error(
        t("project.deleteFailed", {
          error: String(error),
          defaultValue: "删除项目失败：{{error}}",
        }),
        { closeButton: true },
      );
    }
  };

  /** 加载当前选中终端的历史会话列表（Claude Code / OpenCode）。 */
  const loadSessions = async (instance: TerminalInstance) => {
    if (!instance || !SESSION_TOOLS.includes(instance.tool)) return;
    setSessionsLoading(true);
    try {
      const list = await terminalApi.listToolSessions(
        instance.tool,
        instance.projectDir,
      );
      setSessionList(list);
    } catch (error) {
      console.error("[TerminalPanel] failed to load sessions", error);
      setSessionList([]);
      toast.error(
        t("terminalHub.sessionsLoadFailed", {
          error: String(error),
          defaultValue: "加载会话列表失败：{{error}}",
        }),
      );
    } finally {
      setSessionsLoading(false);
    }
  };

  /** 恢复到指定历史会话：更新启动参数并重新初始化终端。 */
  const handleResumeSession = async (session: ToolSessionInfo) => {
    if (!selected) return;
    setSessionsOpen(false);
    const args = mergeSessionArg(
      selected.args ?? "",
      selected.tool,
      session.sessionId,
    );
    hub.updateInstance(selected.id, { args });
    await hub.resetTerminal(selected.id);
    toast.success(
      t("terminalHub.sessionResumed", {
        defaultValue: "已恢复到会话（重启终端中…）",
      }),
    );
  };

  /** 清除会话并重新初始化：按钮 loading 态跟随，避免重复触发与无反馈卡顿。 */
  const handleResetTerminal = async (id: string) => {
    setResettingId(id);
    try {
      await hub.resetTerminal(id);
    } finally {
      setResettingId(null);
    }
  };

  /** 未归属任何项目的终端（兼容旧数据，树形「未分组」节点） */
  const unassignedInstances = instances.filter(
    (item) => !item.projectId || !projects.some((p) => p.id === item.projectId),
  );

  /** 树形子节点：某个分组下的终端列表（支持组内拖拽排序） */
  const renderInstanceGroup = (
    groupInstances: TerminalInstance[],
  ): React.ReactNode => {
    if (groupInstances.length === 0) {
      return (
        <p className="py-0.5 pl-7 pr-1.5 text-[10px] text-muted-foreground/70">
          {t("terminalHub.groupEmpty", { defaultValue: "暂无终端" })}
        </p>
      );
    }
    return (
      <DndContext
        sensors={hub.sensors}
        collisionDetection={closestCenter}
        onDragEnd={handleInstanceDragEnd}
      >
        <SortableContext
          items={groupInstances.map((item) => item.id)}
          strategy={verticalListSortingStrategy}
        >
          <div className="space-y-0.5">
            {groupInstances.map((instance) => {
              const status = instanceAliveStatus(instance, hub);
              const isSelected = selected?.id === instance.id;
              const isContextTarget = contextMenu?.instance.id === instance.id;
              return (
                <SortableTerminalItem
                  key={instance.id}
                  instance={instance}
                  status={status}
                  isSelected={isSelected}
                  isContextTarget={isContextTarget}
                  onSelect={selectInstance}
                  onContextMenu={(event, target) => {
                    event.preventDefault();
                    event.stopPropagation();
                    setContextMenu({
                      x: event.clientX,
                      y: event.clientY,
                      instance: target,
                    });
                  }}
                />
              );
            })}
          </div>
        </SortableContext>
      </DndContext>
    );
  };

  /** 左栏：标题栏 + 项目树（上半）+ 终端列表（下半）；支持折叠为窄条 */
  const renderLeftPanel = () => {
    if (leftCollapsed) {
      return (
        <div
          className="flex shrink-0 flex-col items-center border-r border-border bg-background py-2"
          style={{ width: COLLAPSED_LEFT_WIDTH }}
        >
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7 text-muted-foreground hover:text-foreground"
            onClick={() => setLeftCollapsed(false)}
            title={t("terminalHub.expandLeft", {
              defaultValue: "展开项目与终端",
            })}
          >
            <PanelLeftOpen className="h-4 w-4" />
          </Button>
          {/* 折叠模式精简终端列表：仅 icon，hover 显示名字，点击切换 */}
          {visibleInstances.length > 0 && (
            <div className="mt-2 flex min-h-0 flex-1 flex-col items-center gap-1 overflow-y-auto pb-1">
              {visibleInstances.map((instance) => {
                const status = instanceAliveStatus(instance, hub);
                const isSelected = selected?.id === instance.id;
                return (
                  <Tooltip key={instance.id} delayDuration={200}>
                    <TooltipTrigger asChild>
                      <button
                        type="button"
                        onClick={() => selectInstance(instance)}
                        className={cn(
                          "relative flex h-7 w-7 shrink-0 items-center justify-center rounded-md border transition-colors",
                          isSelected
                            ? "border-primary/50 bg-primary/10"
                            : "border-transparent hover:bg-muted",
                        )}
                      >
                        {APP_ICON_MAP[instance.app as AppId]?.icon ?? (
                          <TerminalSquare className="h-3.5 w-3.5 text-muted-foreground" />
                        )}
                        <span
                          className={cn(
                            "absolute -bottom-0.5 -right-0.5 h-2 w-2 rounded-full ring-2 ring-background",
                            status === "running"
                              ? "bg-emerald-500"
                              : status === "idle"
                                ? "bg-amber-400"
                                : "bg-muted-foreground/40",
                          )}
                        />
                      </button>
                    </TooltipTrigger>
                    <TooltipContent side="right" sideOffset={8}>
                      <span className="block max-w-[200px] truncate text-[11px]">
                        {instance.name}
                      </span>
                    </TooltipContent>
                  </Tooltip>
                );
              })}
            </div>
          )}
          <span
            className="mt-2 text-[10px] font-medium text-muted-foreground"
            style={{ writingMode: "vertical-rl" }}
          >
            {t("terminalHub.projectsTerminals", {
              defaultValue: "项目 · 终端",
            })}
          </span>
        </div>
      );
    }

    return (
      <div
        className="flex shrink-0 flex-col border-r border-border bg-background"
        style={{ width: compact ? COMPACT_LEFT_WIDTH : LEFT_WIDTH }}
      >
        {/* 项目树（上半） */}
        <div className="flex min-h-0 flex-1 flex-col">
          <div className="flex shrink-0 items-center justify-between border-b border-border px-3 py-1.5">
            {/* 项目标题（HUD 拖拽区） */}
            <span
              className="flex select-none items-center gap-1.5 text-[11px] font-medium text-muted-foreground"
              {...(hudDraggable && onHeaderDragStart
                ? { onMouseDown: onHeaderDragStart }
                : {})}
            >
              <FolderKanban className="h-3 w-3" />
              {t("terminalHub.projects", { defaultValue: "项目" })}
            </span>
            <div className="flex items-center gap-0.5">
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6 text-muted-foreground hover:text-foreground"
                title={t("terminalHub.expandAll", {
                  defaultValue: "全部展开",
                })}
                disabled={projects.length === 0}
                onClick={() =>
                  setExpandedIds(new Set(projects.map((item) => item.id)))
                }
              >
                <ChevronsDown className="h-3 w-3" />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6 text-muted-foreground hover:text-foreground"
                title={t("terminalHub.collapseAll", {
                  defaultValue: "全部收起",
                })}
                disabled={expandedIds.size === 0}
                onClick={() => setExpandedIds(new Set())}
              >
                <ChevronsUp className="h-3 w-3" />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6 text-muted-foreground hover:text-foreground"
                title={t("project.newDialog.title", {
                  defaultValue: "新建项目",
                })}
                onClick={() => {
                  setEditingProject(null);
                  setProjectDialogOpen(true);
                }}
              >
                <Plus className="h-3 w-3" />
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6 text-muted-foreground hover:text-foreground"
                onClick={() => setLeftCollapsed(true)}
                title={t("terminalHub.collapseLeft", {
                  defaultValue: "折叠左栏",
                })}
              >
                <PanelLeftClose className="h-3 w-3" />
              </Button>
            </div>
          </div>
          <ScrollArea className="min-h-0 flex-1">
            <div className="p-1.5">
              {/* 项目树：项目 → 终端 两级结构（点击项目应用并展开） */}
              {projects.length === 0 ? (
                <div className="rounded-lg border border-dashed border-border px-3 py-6 text-center text-[11px] text-muted-foreground">
                  {t("terminalHub.noProjects", {
                    defaultValue: "还没有项目，点击右上角 + 新建一个",
                  })}
                </div>
              ) : (
                <div className="space-y-0.5">
                  {projects.map((project) => {
                    const isActive = selectedProject?.id === project.id;
                    const toolCount = Object.keys(project.tools ?? {}).length;
                    const expanded = expandedIds.has(project.id);
                    const projectTerminals = instances.filter(
                      (item) => item.projectId === project.id,
                    );
                    return (
                      <div key={project.id}>
                        <div
                          role="button"
                          tabIndex={0}
                          onClick={() => {
                            if (!expanded) toggleExpanded(project.id);
                            void handleApplyProject(project.id);
                          }}
                          onKeyDown={(event) => {
                            if (event.key === "Enter" || event.key === " ") {
                              event.preventDefault();
                              void handleApplyProject(project.id);
                            }
                          }}
                          onContextMenu={(event) => {
                            event.preventDefault();
                            event.stopPropagation();
                            setProjectContextMenu({
                              x: event.clientX,
                              y: event.clientY,
                              project,
                            });
                          }}
                          className={cn(
                            "group flex w-full cursor-pointer items-center gap-1 rounded-md px-1 py-1 text-left transition-colors",
                            isActive
                              ? "bg-primary/5 font-medium text-primary"
                              : "hover:bg-muted/40",
                          )}
                        >
                          <button
                            type="button"
                            className="flex h-4 w-4 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-muted"
                            onClick={(event) => {
                              event.stopPropagation();
                              toggleExpanded(project.id);
                            }}
                            title={
                              expanded
                                ? t("terminalHub.collapse", {
                                    defaultValue: "收起",
                                  })
                                : t("terminalHub.expand", {
                                    defaultValue: "展开",
                                  })
                            }
                          >
                            <ChevronRight
                              className={cn(
                                "h-3 w-3 transition-transform",
                                expanded && "rotate-90",
                              )}
                            />
                          </button>
                          <FolderKanban
                            className={cn(
                              "h-3.5 w-3.5 shrink-0",
                              projectBusy && isActive
                                ? "animate-spin text-primary"
                                : "text-muted-foreground",
                            )}
                          />
                          <span className="min-w-0 flex-1 truncate text-xs font-medium">
                            {project.name}
                          </span>
                          {toolCount > 0 && (
                            <span className="shrink-0 rounded bg-muted px-1 py-0.5 text-[9px] text-muted-foreground">
                              {toolCount}
                            </span>
                          )}
                          {/* 行内操作：新建终端（归属该项目）；编辑/删除在右键菜单 */}
                          <button
                            type="button"
                            className="flex h-4 w-4 shrink-0 items-center justify-center rounded text-muted-foreground/60 transition-colors hover:bg-muted hover:text-foreground"
                            onClick={(event) => {
                              event.stopPropagation();
                              setNewTerminalProjectId(project.id);
                              setNewOpen(true);
                            }}
                            title={t("terminalHub.newTerminalFor", {
                              name: project.name,
                              defaultValue: `为「${project.name}」新建终端`,
                            })}
                          >
                            <Plus className="h-3 w-3" />
                          </button>
                        </div>
                        {expanded && (
                          <div className="pb-0.5 pl-3.5 pr-0.5">
                            {renderInstanceGroup(projectTerminals)}
                          </div>
                        )}
                      </div>
                    );
                  })}
                  {/* 未归属项目的终端（兼容旧数据） */}
                  {unassignedInstances.length > 0 && (
                    <div>
                      <div
                        role="button"
                        tabIndex={0}
                        onClick={() => toggleExpanded(UNASSIGNED_GROUP_ID)}
                        className="flex w-full cursor-pointer items-center gap-1 rounded-md px-1 py-1 text-left transition-colors hover:bg-muted/40"
                      >
                        <button
                          type="button"
                          className="flex h-4 w-4 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-muted"
                          onClick={(event) => {
                            event.stopPropagation();
                            toggleExpanded(UNASSIGNED_GROUP_ID);
                          }}
                        >
                          <ChevronRight
                            className={cn(
                              "h-3 w-3 transition-transform",
                              expandedIds.has(UNASSIGNED_GROUP_ID) &&
                                "rotate-90",
                            )}
                          />
                        </button>
                        <TerminalSquare className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                        <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">
                          {t("terminalHub.unassigned", {
                            defaultValue: "未分组终端",
                          })}
                        </span>
                        <span className="shrink-0 rounded bg-muted px-1 py-0.5 text-[9px] text-muted-foreground">
                          {unassignedInstances.length}
                        </span>
                      </div>
                      {expandedIds.has(UNASSIGNED_GROUP_ID) && (
                        <div className="pb-0.5 pl-3.5 pr-0.5">
                          {renderInstanceGroup(unassignedInstances)}
                        </div>
                      )}
                    </div>
                  )}
                </div>
              )}
            </div>
          </ScrollArea>
        </div>

        {/* 底部：Git 更改文件 */}
        {!selectOnly && (
          <div className="flex h-56 shrink-0 flex-col border-t border-border">
            <div className="flex shrink-0 items-center justify-between border-b border-border px-3 py-1.5">
              <span className="flex items-center gap-1.5 text-[11px] font-medium text-muted-foreground">
                <GitBranch className="h-3 w-3" />
                {t("terminalHub.gitChanges", { defaultValue: "Git 更改" })}
                {gitStatus?.git && gitStatus.entries.length > 0
                  ? ` (${gitStatus.entries.length})`
                  : ""}
              </span>
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6 shrink-0 text-muted-foreground hover:text-foreground"
                onClick={() => void loadGitStatus()}
                title={t("terminalHub.gitRefresh", { defaultValue: "刷新" })}
              >
                <RotateCcw className="h-3 w-3" />
              </Button>
            </div>
            <div className="min-h-0 flex-1 overflow-y-auto p-1.5">
              {!selectedProject ? (
                <p className="px-1 py-2 text-[11px] text-muted-foreground">
                  {t("terminalHub.gitNoProject", {
                    defaultValue: "选择一个项目后显示其 Git 更改",
                  })}
                </p>
              ) : !gitStatus ? (
                <p className="px-1 py-2 text-[11px] text-muted-foreground">
                  {t("terminalHub.gitLoading", { defaultValue: "加载中…" })}
                </p>
              ) : !gitStatus.git ? (
                <p className="px-1 py-2 text-[11px] text-muted-foreground">
                  {t("terminalHub.gitNoRepo", {
                    defaultValue: "当前项目不是 Git 仓库",
                  })}
                </p>
              ) : gitStatus.error ? (
                <p className="px-1 py-2 text-[11px] text-red-500/80">
                  {gitStatus.error}
                </p>
              ) : gitStatus.entries.length === 0 ? (
                <p className="px-1 py-2 text-[11px] text-muted-foreground">
                  {t("terminalHub.gitClean", {
                    defaultValue: "工作区没有未提交的更改",
                  })}
                </p>
              ) : (
                <div className="space-y-0.5">
                  {gitStatus.entries.map((entry) => (
                    <div
                      key={`${entry.raw}-${entry.path}`}
                      className="flex min-w-0 cursor-pointer items-center gap-1.5 rounded px-1.5 py-0.5 hover:bg-muted/40"
                      title={`${gitStatusLabel(entry.raw)} · ${entry.path}`}
                      onClick={() => {
                        if (!selectedProject) return;
                        setEditorFile({
                          filePath: `${selectedProject.projectDir}/${entry.path}`,
                          projectDir: selectedProject.projectDir,
                        });
                      }}
                    >
                      <span
                        className={cn(
                          "w-4 shrink-0 text-center font-mono text-[9px]",
                          gitStatusColor(entry.raw),
                        )}
                      >
                        {entry.raw.trim() || "·"}
                      </span>
                      <span className="min-w-0 flex-1 truncate font-mono text-[10px] text-foreground/80">
                        {entry.path}
                      </span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    );
  };

  /** 中栏：终端头部 + 终端主体 */
  const renderTerminalArea = () => (
    <div className="flex min-w-0 flex-1 flex-col bg-background">
      {/* 终端头部 */}
      <div className="flex shrink-0 items-center gap-2 border-b border-border bg-muted/50 px-2.5 py-1.5">
        {selected && (
          <span className="flex min-w-0 items-center gap-1.5 text-foreground">
            {APP_ICON_MAP[selected.app as AppId]?.icon}
            <span className="truncate text-xs font-medium">
              {selected.name}
            </span>
          </span>
        )}
        <span className="min-w-0 flex-1 truncate px-1 font-mono text-[10px] text-muted-foreground">
          {selected?.projectDir ?? appLabel}
        </span>
        {/* 实时统计：输出速率（生成中实时/上次请求）+ 上下文长度 + 会话用量 */}
        <span
          className="inline-flex shrink-0 items-center gap-1.5 rounded-md border border-border/60 bg-background/60 px-1.5 py-0.5 text-[10px] tabular-nums text-muted-foreground"
          title={
            modelRate &&
            (modelRate.lastInputTokens > 0 ||
              modelRate.sessionInputTokens > 0 ||
              modelRate.totalTokens > 0)
              ? t("terminalHub.statsDetail", {
                  model: modelRate.lastModel || "未知模型",
                  window: modelRate.sampleSeconds,
                  output: formatTokenCount(modelRate.outputTokens),
                  context: formatTokenCount(modelRate.lastInputTokens),
                  sessionIn: formatTokenCount(modelRate.sessionInputTokens),
                  sessionOut: formatTokenCount(modelRate.sessionOutputTokens),
                  cache: formatTokenCount(modelRate.sessionCacheTokens),
                  total: formatTokenCount(modelRate.totalTokens),
                  duration: formatDuration(modelRate.lastDurationMs),
                })
              : t("terminalHub.statsIdle")
          }
        >
          <Gauge className="h-3 w-3 shrink-0 text-emerald-500/80" />
          {/* 当前/最近模型（tooltip 中有完整信息，这里只放短名） */}
          {modelRate?.lastModel && (
            <>
              <span
                className="max-w-[11rem] truncate font-medium text-foreground/80"
                title={modelRate.lastModel}
              >
                {formatModelName(modelRate.lastModel)}
              </span>
              <span className="h-2.5 w-px shrink-0 bg-border" />
            </>
          )}
          {modelRate?.generating ? (
            <span className="inline-flex shrink-0 items-center gap-1 text-amber-500">
              <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-amber-500" />
              {t("terminalHub.statsLiveSpeed", {
                speed: formatSpeed(modelRate.liveSpeedTokS),
                defaultValue: "~{{speed}} tok/s",
              })}
            </span>
          ) : modelRate && modelRate.lastDurationMs > 0 ? (
            <span className="shrink-0">
              {t("terminalHub.statsSpeed", {
                speed: formatSpeed(modelRate.lastSpeedTokS),
                defaultValue: "上次 {{speed}} tok/s",
              })}
            </span>
          ) : (
            <span className="shrink-0">-- tok/s</span>
          )}
          {modelRate && modelRate.lastInputTokens > 0 && (
            <>
              <span className="h-2.5 w-px bg-border" />
              {t("terminalHub.statsContext", {
                tokens: formatTokenCount(modelRate.lastInputTokens),
                defaultValue: "上下文 {{tokens}}",
              })}
            </>
          )}
          {modelRate &&
            (modelRate.sessionInputTokens > 0 ||
              modelRate.sessionOutputTokens > 0) && (
              <>
                <span className="h-2.5 w-px bg-border" />
                {t("terminalHub.statsSessionUsage", {
                  input: formatTokenCount(modelRate.sessionInputTokens),
                  output: formatTokenCount(modelRate.sessionOutputTokens),
                  defaultValue: "用量 {{input}}/{{output}}",
                })}
              </>
            )}
        </span>
        {onHudRestore && (
          <Button
            variant="ghost"
            size="icon"
            className="h-6 w-6 shrink-0 text-muted-foreground hover:bg-muted hover:text-foreground"
            title={t("terminalHub.restoreFullscreen", {
              defaultValue: "还原大屏幕",
            })}
            onClick={onHudRestore}
          >
            <Maximize2 className="h-3.5 w-3.5" />
          </Button>
        )}
        {selected && SESSION_TOOLS.includes(selected.tool) && (
          <DropdownMenu
            open={sessionsOpen}
            onOpenChange={(open) => {
              setSessionsOpen(open);
              if (open && selected) void loadSessions(selected);
            }}
          >
            <DropdownMenuTrigger asChild>
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6 shrink-0 text-muted-foreground hover:bg-muted hover:text-foreground"
                title={t("terminalHub.resumeSession", {
                  defaultValue: "恢复历史会话",
                })}
              >
                <History className="h-3.5 w-3.5" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent
              align="end"
              className="max-h-80 w-72 overflow-y-auto"
            >
              <DropdownMenuLabel className="text-xs text-muted-foreground">
                {t("terminalHub.recentSessions", {
                  defaultValue: "历史会话（点击恢复）",
                })}
              </DropdownMenuLabel>
              <DropdownMenuSeparator className="bg-border" />
              {sessionsLoading ? (
                <div className="flex items-center justify-center py-4">
                  <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
                </div>
              ) : sessionList.length === 0 ? (
                <p className="px-3 py-3 text-center text-[11px] text-muted-foreground">
                  {t("terminalHub.noSessions", {
                    defaultValue: "未找到可恢复的会话",
                  })}
                </p>
              ) : (
                sessionList.map((session) => (
                  <DropdownMenuItem
                    key={session.sessionId}
                    onSelect={() => void handleResumeSession(session)}
                    className="flex min-w-0 flex-col items-start gap-0.5"
                  >
                    <span className="max-w-full truncate text-xs text-foreground">
                      {session.title?.trim() || session.sessionId}
                    </span>
                    <span className="text-[10px] text-muted-foreground">
                      {formatSessionTime(session.lastActiveAt) ||
                        session.projectDir ||
                        ""}
                    </span>
                  </DropdownMenuItem>
                ))
              )}
            </DropdownMenuContent>
          </DropdownMenu>
        )}
        <Button
          variant="ghost"
          size="icon"
          className="h-6 w-6 shrink-0 text-muted-foreground hover:bg-muted hover:text-foreground"
          title={t("terminalHub.reset", {
            defaultValue: "清除会话并重新初始化（全新终端）",
          })}
          onClick={() => selected && void handleResetTerminal(selected.id)}
          disabled={resettingId !== null}
        >
          {resettingId === selected?.id ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <RotateCcw className="h-3.5 w-3.5" />
          )}
        </Button>
        <Button
          variant="ghost"
          size="icon"
          className="h-6 w-6 shrink-0 text-muted-foreground hover:bg-muted hover:text-foreground"
          title={t("terminalHub.stop", { defaultValue: "停止" })}
          onClick={() => selected && void hub.stopTerminal(selected.id)}
          disabled={!activeRunning}
        >
          <Square className="h-3.5 w-3.5" />
        </Button>
      </div>

      {/* 终端主体：面板内嵌 PTY 终端（xterm.js）。
          已激活的终端常驻挂载，非选中项以 display:none 隐藏——切换标签时
          xterm 屏幕状态原样保留，无需回放 PTY 历史即可完整恢复显示
          （全屏 TUI 会话靠字节回放无法可靠还原屏幕）。 */}
      <div className="min-h-0 flex-1 p-1">
        {selected && activatedInstances.length > 0 ? (
          <Suspense
            fallback={
              <div className="flex h-full items-center justify-center text-xs text-muted-foreground">
                <Loader2 className="mr-2 h-3.5 w-3.5 animate-spin" />
                {t("terminalHub.loadingEmbedded", {
                  defaultValue: "正在启动终端…",
                })}
              </div>
            }
          >
            {activatedInstances.map((instance) => {
              const active = instance.id === selectedId;
              return (
                <div
                  key={instance.id}
                  className={cn("h-full", !active && "hidden")}
                >
                  <EmbeddedTerminalLazy
                    instance={instance}
                    active={active}
                    resetNonce={hub.resetNonce[instance.id] ?? 0}
                    onPtyChange={(ptyId) =>
                      hub.registerEmbedded(instance.id, ptyId)
                    }
                    onExitChange={(exited) =>
                      hub.setEmbeddedExited(instance.id, exited)
                    }
                  />
                </div>
              );
            })}
          </Suspense>
        ) : (
          <div className="flex h-full flex-col items-center justify-center gap-3 text-muted-foreground">
            <TerminalSquare className="h-8 w-8 opacity-40" />
            <p className="px-6 text-center text-xs leading-relaxed">
              {t("terminalHub.emptyRight", {
                defaultValue: "点击左侧「新建终端」创建一个终端",
              })}
            </p>
          </div>
        )}
      </div>
    </div>
  );

  return (
    <TooltipProvider delayDuration={200}>
      {/* 主布局：三栏式终端工作台（左：项目树+终端列表 / 中：终端 / 右：模型列表） */}
      <div className="flex h-full w-full overflow-hidden bg-background text-foreground">
        {renderLeftPanel()}
        {renderTerminalArea()}
        {/* 右栏：模型列表（供应商列表）；支持折叠为窄条（与左栏一致） */}
        {rightPanel &&
          (rightCollapsed ? (
            <div
              className="flex shrink-0 flex-col items-center border-l border-border bg-background py-2"
              style={{ width: COLLAPSED_LEFT_WIDTH }}
            >
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7 text-muted-foreground hover:text-foreground"
                onClick={() => setRightCollapsed(false)}
                title={t("terminalHub.expandRight", {
                  defaultValue: "展开模型列表",
                })}
              >
                <PanelRightOpen className="h-4 w-4" />
              </Button>
              <span
                className="mt-2 text-[10px] font-medium text-muted-foreground"
                style={{ writingMode: "vertical-rl" }}
              >
                {t("provider.title", { defaultValue: "模型列表" })}
              </span>
            </div>
          ) : (
            <div
              className="group/right relative flex shrink-0 flex-col border-l border-border bg-background"
              style={{ width: RIGHT_WIDTH }}
            >
              {/* 折叠按钮：hover 显现，避免遮挡顶行的用量信息 */}
              <Button
                variant="ghost"
                size="icon"
                className="absolute right-1.5 top-1.5 z-10 h-6 w-6 opacity-0 transition-opacity group-hover/right:opacity-100 text-muted-foreground hover:text-foreground"
                onClick={() => setRightCollapsed(true)}
                title={t("terminalHub.collapseRight", {
                  defaultValue: "折叠模型列表",
                })}
              >
                <PanelRightClose className="h-3 w-3" />
              </Button>
              {rightPanel}
            </div>
          ))}
      </div>

      {contextMenu &&
        (() => {
          const { x, y, instance } = contextMenu;
          const status = instanceAliveStatus(instance, hub);
          const menuWidth = 176;
          const menuHeight = 186;
          const left = Math.max(
            8,
            Math.min(x, window.innerWidth - menuWidth - 8),
          );
          const top = Math.max(
            8,
            Math.min(y, window.innerHeight - menuHeight - 8),
          );
          return (
            <div
              className="fixed z-50 w-44 overflow-hidden rounded-lg border border-border bg-popover p-1 shadow-xl"
              style={{ left, top }}
              onContextMenu={(event) => event.preventDefault()}
            >
              <button
                type="button"
                onClick={() => {
                  setContextMenu(null);
                  if (status === "running") {
                    void hub.focusTerminal(instance.id);
                  } else {
                    void hub.openTerminal(instance.id);
                  }
                }}
                className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-foreground transition-colors hover:bg-muted"
              >
                <Play className="h-3.5 w-3.5 text-muted-foreground" />
                {status === "running" ? "聚焦窗口" : "打开终端"}
              </button>
              <button
                type="button"
                disabled={status !== "running"}
                onClick={() => {
                  setContextMenu(null);
                  void hub.stopTerminal(instance.id);
                }}
                className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-foreground transition-colors hover:bg-muted disabled:opacity-40"
              >
                <Square className="h-3.5 w-3.5 text-muted-foreground" />
                {t("terminalHub.stop", { defaultValue: "停止" })}
              </button>
              <button
                type="button"
                onClick={() => {
                  setContextMenu(null);
                  void handleResetTerminal(instance.id);
                }}
                className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-foreground transition-colors hover:bg-muted"
              >
                <RotateCcw className="h-3.5 w-3.5 text-muted-foreground" />
                {t("terminalHub.reset", {
                  defaultValue: "重置会话",
                })}
              </button>
              {SESSION_TOOLS.includes(instance.tool) && (
                <button
                  type="button"
                  onClick={() => {
                    setContextMenu(null);
                    const next = instance.autoResume === false;
                    hub.updateInstance(instance.id, { autoResume: next });
                    toast.success(
                      next
                        ? "已开启：重启应用后自动恢复该终端会话"
                        : "已关闭：重启应用后开全新会话",
                    );
                  }}
                  className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-foreground transition-colors hover:bg-muted"
                  title="控制重启应用后是否自动恢复该终端的会话与内容"
                >
                  <History className="h-3.5 w-3.5 text-muted-foreground" />
                  {instance.autoResume === false
                    ? "开启重启自动恢复"
                    : "关闭重启自动恢复"}
                </button>
              )}
              <div className="my-1 h-px bg-border" />
              <button
                type="button"
                onClick={() => {
                  setContextMenu(null);
                  setConfirmDelete(instance);
                }}
                className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-red-500 transition-colors hover:bg-red-500/10"
              >
                <Trash2 className="h-3.5 w-3.5" />
                {t("terminalHub.delete", { defaultValue: "删除" })}
              </button>
            </div>
          );
        })()}

      {projectContextMenu &&
        (() => {
          const { x, y, project } = projectContextMenu;
          const menuWidth = 176;
          const menuHeight = 112;
          const left = Math.max(
            8,
            Math.min(x, window.innerWidth - menuWidth - 8),
          );
          const top = Math.max(
            8,
            Math.min(y, window.innerHeight - menuHeight - 8),
          );
          return (
            <div
              className="fixed z-50 w-44 overflow-hidden rounded-lg border border-border bg-popover p-1 shadow-xl"
              style={{ left, top }}
              onContextMenu={(event) => event.preventDefault()}
            >
              <button
                type="button"
                onClick={() => {
                  setProjectContextMenu(null);
                  setEditingProject(project);
                  setProjectDialogOpen(true);
                }}
                className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-foreground transition-colors hover:bg-muted"
              >
                <Pencil className="h-3.5 w-3.5 text-muted-foreground" />
                {t("project.newDialog.editTitle", {
                  defaultValue: "编辑项目",
                })}
              </button>
              <div className="my-1 h-px bg-border" />
              <button
                type="button"
                onClick={() => {
                  setProjectContextMenu(null);
                  setConfirmDeleteProject(project);
                }}
                className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs text-red-500 transition-colors hover:bg-red-500/10"
              >
                <Trash2 className="h-3.5 w-3.5" />
                {t("project.delete", { defaultValue: "删除项目" })}
              </button>
            </div>
          );
        })()}

      {newOpen && (
        <NewTerminalDialog
          open={newOpen}
          onOpenChange={(open) => {
            setNewOpen(open);
            if (!open) setNewTerminalProjectId(null);
          }}
          appId={effectiveApp}
          appLabel={appLabel}
          projectDirs={hub.state.projectDirs}
          defaultProjectDir={
            projects.find((item) => item.id === newTerminalProjectId)
              ?.projectDir ??
            selectedProject?.projectDir ??
            hub.state.lastProjectDir
          }
          projectId={newTerminalProjectId ?? selectedProject?.id}
          availableTerminals={hub.availableTerminals}
          launchTemplates={launchTemplates}
          onBrowse={() => handleBrowse()}
          onListSessions={(tool, projectDir) =>
            terminalApi.listToolSessions(tool, projectDir)
          }
          onSubmit={async (payload) => {
            // 新建即选中：创建成功后直接切到新终端并激活会话，
            // 同时展开它所属的项目分组（否则树里看不见新条目）
            const createdId = await hub.createTerminal(payload);
            if (createdId) {
              const groupId = payload.projectId;
              if (groupId) {
                setExpandedIds((prev) => new Set(prev).add(groupId));
              }
              setSelectedId(createdId);
              hub.setActiveInstanceId(createdId);
            }
            return createdId;
          }}
        />
      )}

      {projectDialogOpen && (
        <NewProjectDialog
          open={projectDialogOpen}
          onOpenChange={(open) => {
            setProjectDialogOpen(open);
            if (!open) setEditingProject(null);
          }}
          projectDirs={hub.state.projectDirs}
          defaultProjectDir={
            selectedProject?.projectDir ?? hub.state.lastProjectDir
          }
          project={editingProject}
          onBrowse={() => handleBrowse()}
          onCreated={() => void loadProjects()}
        />
      )}

      {/* Git 更改点击 → 差异 / 编辑 */}
      <FileEditorDialog
        open={Boolean(editorFile)}
        filePath={editorFile?.filePath ?? ""}
        projectDir={editorFile?.projectDir ?? ""}
        initialView="diff"
        onClose={() => setEditorFile(null)}
      />

      <ConfirmDialog
        isOpen={Boolean(confirmDelete)}
        pending={deletingId !== null}
        title={t("terminalHub.deleteConfirmTitle", {
          defaultValue: "删除终端",
        })}
        message={t("terminalHub.deleteConfirmMessage", {
          defaultValue: "确定要删除终端「{{name}}」吗？",
          name: confirmDelete?.name ?? "",
        })}
        onConfirm={async () => {
          if (!confirmDelete) return;
          setDeletingId(confirmDelete.id);
          // 标记删除进行中：删除选中终端后不自动切换/启动下一个终端，
          // 避免删除瞬间面板弹出未预期的终端会话（由用户点击列表项启动）
          deletingRef.current = confirmDelete.id;
          try {
            await hub.deleteTerminal(confirmDelete.id);
          } finally {
            setDeletingId(null);
            setConfirmDelete(null);
          }
        }}
        onCancel={() => setConfirmDelete(null)}
      />

      <ConfirmDialog
        isOpen={Boolean(confirmDeleteProject)}
        title={t("project.deleteConfirmTitle", {
          defaultValue: "删除项目",
        })}
        message={t("project.deleteConfirmMessage", {
          defaultValue:
            "确定要删除项目「{{name}}」吗？快照（MCP / Skills / CLAUDE.md）将一并删除。",
          name: confirmDeleteProject?.name ?? "",
        })}
        confirmText={t("project.delete", { defaultValue: "删除" })}
        onConfirm={() => void handleDeleteProject()}
        onCancel={() => setConfirmDeleteProject(null)}
      />
    </TooltipProvider>
  );
}
