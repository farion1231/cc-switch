import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  defaultRangeExtractor,
  observeElementRect,
  useVirtualizer,
  type Range,
} from "@tanstack/react-virtual";
import { toast } from "@/lib/toast";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Check,
  ChevronDown,
  History,
  LayoutGrid,
  MoreHorizontal,
} from "lucide-react";
import { useSessionSearch } from "@/hooks/useSessionSearch";
import {
  piKeys,
  useDeleteSessionMutation,
  useSessionsQuery,
  useSettingsQuery,
} from "@/lib/query";
import { piApi, sessionsApi, type AppId } from "@/lib/api";
import type { SessionMeta } from "@/types";
import { DEFAULT_VISIBLE_APPS } from "@/config/appConfig";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import { Notice } from "@/components/ui/notice";
import { SegmentedControl } from "@/components/ui/segmented-control";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { HoverTip } from "@/components/ui/hover-tip";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { AppGlyph, APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { extractErrorMessage } from "@/utils/errorUtils";
import { isMac } from "@/lib/platform";
import { SearchField } from "@/components/ui/search-field";
import { cn } from "@/lib/utils";
import { SessionItem, sessionMenuItemClass } from "./SessionItem";
import { SessionReader } from "./reader/SessionReader";
import { sessionKeys, useSessionTranscript } from "@/lib/query/sessions";
import { SessionDeleteDialog, SessionSourcesDialog } from "./SessionDialogs";
import {
  canDeleteSession,
  formatRelativeTime,
  getSessionKey,
  groupSessionsByProject,
  groupSessionsByTime,
  isSessionAppId,
  SESSION_APP_IDS,
  SESSION_SOURCE_PATHS,
  shortenHomePath,
  sortSessionsByTime,
  type SessionAppId,
  type SessionTimeBucket,
} from "./utils";

const GROUP_MODE_STORAGE_KEY = "cc-switch.sessionManager.groupMode";
// 按项目分组时默认全部收起，只记住用户手动展开过的项目。
// 换了新键：旧的 collapsedProjects 记的是「收起了哪些」，语义相反，直接弃用。
const EXPANDED_STORAGE_KEY = "cc-switch.sessionManager.expandedProjects";

type AppFilter = SessionAppId | "all";
type GroupMode = "time" | "project";
type DeleteSource = "row" | "reader" | "bar";

const readGroupMode = (): GroupMode => {
  try {
    const stored = window.localStorage.getItem(GROUP_MODE_STORAGE_KEY);
    return stored === "time" ? "time" : "project";
  } catch {
    return "project";
  }
};

const readExpanded = (): Set<string> => {
  try {
    const parsed = JSON.parse(
      window.localStorage.getItem(EXPANDED_STORAGE_KEY) ?? "[]",
    );
    return new Set(
      Array.isArray(parsed)
        ? parsed.filter((v): v is string => typeof v === "string")
        : [],
    );
  } catch {
    return new Set();
  }
};

const openButtonId = (key: string) => `session-open-${encodeURIComponent(key)}`;

const focusSoon = (ids: string[]) => {
  window.setTimeout(() => {
    for (const id of ids) {
      const el = document.getElementById(id);
      if (el) {
        el.focus();
        return;
      }
    }
  }, 0);
};

type SessionListRow =
  | { kind: "time"; key: string; bucket: SessionTimeBucket; first: boolean }
  | {
      kind: "project";
      key: string;
      group: ReturnType<typeof groupSessionsByProject>[number];
      first: boolean;
    }
  | { kind: "session"; key: string; session: SessionMeta; bordered: boolean };

/** 虚拟列表每种行的固定高度：时间分组标题 h-7、项目分组标题 h-9、会话行 h-14 */
const LIST_ROW_HEIGHT = { time: 28, project: 36, session: 56 } as const;

const linkButton =
  "h-[22px] rounded-[4px] px-0.5 text-caption text-fg-1 underline decoration-border-strong underline-offset-[3px] hover:decoration-fg-2";

interface SessionManagerPageProps {
  appId: string;
  /** 侧栏上当前的应用（从 Claude Desktop 进来时要提示一句） */
  fromApp?: AppId;
  /** 打开设置里的「首选终端」 */
  onOpenTerminalSettings?: () => void;
}

export function SessionManagerPage({
  appId,
  fromApp,
  onOpenTerminalSettings,
}: SessionManagerPageProps) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const { data, isLoading, refetch } = useSessionsQuery();
  const sessions = useMemo(() => data ?? [], [data]);
  const { data: settings } = useSettingsQuery();

  const [appFilter, setAppFilter] = useState<AppFilter>(
    isSessionAppId(appId) ? appId : "claude",
  );
  const [query, setQuery] = useState("");
  const [groupMode, setGroupMode] = useState<GroupMode>(readGroupMode);
  const [expanded, setExpanded] = useState<Set<string>>(readExpanded);
  const [readerKey, setReaderKey] = useState<string | null>(null);
  const [selectionMode, setSelectionMode] = useState(false);
  const [selectedKeys, setSelectedKeys] = useState<Set<string>>(
    () => new Set(),
  );
  const [deleteTargets, setDeleteTargets] = useState<SessionMeta[] | null>(
    null,
  );
  const deleteSourceRef = useRef<DeleteSource>("row");
  const [isBatchDeleting, setIsBatchDeleting] = useState(false);
  const [whereOpen, setWhereOpen] = useState(false);
  const searchRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    setAppFilter(isSessionAppId(appId) ? appId : "claude");
  }, [appId]);

  useEffect(() => {
    try {
      window.localStorage.setItem(GROUP_MODE_STORAGE_KEY, groupMode);
    } catch {
      // 存不了就算了，下次用默认值
    }
  }, [groupMode]);

  useEffect(() => {
    try {
      window.localStorage.setItem(
        EXPANDED_STORAGE_KEY,
        JSON.stringify(Array.from(expanded).sort()),
      );
    } catch {
      // 同上
    }
  }, [expanded]);

  const piSessionDiscovery = useQuery({
    queryKey: piKeys.sessionDiscovery,
    queryFn: () => piApi.getSessionDiscovery(),
    enabled: appFilter === "pi",
    staleTime: 30 * 1000,
  });

  // 下拉里只列「应用」页设为显示的应用；Claude Code 的会话在 Claude Desktop 显示时也算
  const availableApps = useMemo(() => {
    const visible = { ...DEFAULT_VISIBLE_APPS, ...settings?.visibleApps };
    return SESSION_APP_IDS.filter(
      (app) =>
        visible[app] ||
        (app === "claude" && visible["claude-desktop"]) ||
        app === appFilter,
    );
  }, [settings?.visibleApps, appFilter]);

  const scopedSessions = useMemo(
    () =>
      sessions.filter((session) =>
        (availableApps as string[]).includes(session.providerId),
      ),
    [sessions, availableApps],
  );

  const counts = useMemo(() => {
    const map = new Map<string, number>();
    scopedSessions.forEach((session) =>
      map.set(session.providerId, (map.get(session.providerId) ?? 0) + 1),
    );
    return map;
  }, [scopedSessions]);

  const menuApps = useMemo(
    () =>
      [...availableApps].sort((a, b) => {
        const diff = (counts.get(b) ?? 0) - (counts.get(a) ?? 0);
        return diff || SESSION_APP_IDS.indexOf(a) - SESSION_APP_IDS.indexOf(b);
      }),
    [availableApps, counts],
  );

  const { search: searchSessions } = useSessionSearch({
    sessions: scopedSessions,
    providerFilter: appFilter,
  });
  const trimmedQuery = query.trim();
  // 搜索时全部展开，否则匹配到的会话会藏在收起的项目里
  const isGroupOpen = useCallback(
    (key: string) => trimmedQuery !== "" || expanded.has(key),
    [trimmedQuery, expanded],
  );
  const matches = useMemo(
    () => sortSessionsByTime(searchSessions(query)),
    [searchSessions, query],
  );

  const unknownLabel = t("sessionManager.unknownDirectory", {
    defaultValue: "未知目录",
  });
  const projectGroups = useMemo(
    () =>
      groupMode === "project"
        ? groupSessionsByProject(matches, unknownLabel)
        : [],
    [groupMode, matches, unknownLabel],
  );
  const timeGroups = useMemo(
    () => (groupMode === "time" ? groupSessionsByTime(matches) : []),
    [groupMode, matches],
  );
  // 阅读页的「上一个 / 下一个」按列表上的顺序走
  const orderedSessions = useMemo(
    () =>
      groupMode === "project"
        ? projectGroups.flatMap((group) => group.sessions)
        : matches,
    [groupMode, projectGroups, matches],
  );

  const readerSession = useMemo(
    () =>
      readerKey
        ? (sessions.find((session) => getSessionKey(session) === readerKey) ??
          null)
        : null,
    [sessions, readerKey],
  );
  const readerIndex = readerSession
    ? orderedSessions.findIndex(
        (session) => getSessionKey(session) === readerKey,
      )
    : -1;

  // 阅读中的会话被删掉（或刷新后消失）就回到列表
  useEffect(() => {
    if (readerKey && !isLoading && !readerSession) {
      setReaderKey(null);
    }
  }, [readerKey, readerSession, isLoading]);

  const transcript = useSessionTranscript(
    readerSession?.providerId,
    readerSession?.sourcePath,
  );

  const deleteSessionMutation = useDeleteSessionMutation();
  const isDeleting = deleteSessionMutation.isPending || isBatchDeleting;

  // 选择里去掉已经不存在的会话
  useEffect(() => {
    const valid = new Set(sessions.map(getSessionKey));
    setSelectedKeys((current) => {
      const next = new Set([...current].filter((key) => valid.has(key)));
      return next.size === current.size ? current : next;
    });
  }, [sessions]);

  const terminalName = useMemo(() => {
    if (!isMac()) return null;
    const value = settings?.preferredTerminal || "terminal";
    const key = `settings.terminal.options.macos.${value}`;
    const label = t(key);
    return label === key ? value : label;
  }, [settings?.preferredTerminal, t]);

  const copyText = useCallback(
    async (text: string, message: string) => {
      try {
        await navigator.clipboard.writeText(text);
        toast.success(message);
      } catch (error) {
        toast.error(
          extractErrorMessage(error) ||
            t("common.error", { defaultValue: "Copy failed" }),
        );
      }
    },
    [t],
  );
  const handleCopy = useCallback(
    (text: string, message: string) => void copyText(text, message),
    [copyText],
  );

  const handleLaunch = async (session: SessionMeta) => {
    const command = session.resumeCommand;
    if (!command) return;
    if (!terminalName) {
      handleCopy(
        command,
        t("sessionManager.resumeCopiedNoLaunch", {
          defaultValue:
            "已复制恢复命令。这个平台暂不支持一键恢复，请在项目目录下粘贴到终端运行",
        }),
      );
      return;
    }
    try {
      await sessionsApi.launchTerminal({
        command,
        cwd: session.projectDir ?? undefined,
      });
      toast.success(
        t("sessionManager.launchedIn", {
          defaultValue: "已在 {{terminal}} 中运行恢复命令",
          terminal: terminalName,
        }),
      );
    } catch (error) {
      try {
        await navigator.clipboard.writeText(command);
      } catch {
        // 复制失败时错误信息里照样给出原因
      }
      toast.error(
        t("sessionManager.launchFailed", {
          defaultValue:
            "无法打开 {{terminal}}：{{error}}。恢复命令已复制，可以粘贴到任意终端运行。",
          terminal: terminalName,
          error:
            extractErrorMessage(error) ||
            t("common.unknown", { defaultValue: "未知" }),
        }),
        onOpenTerminalSettings
          ? {
              action: {
                label: t("sessionManager.changeTerminalAction", {
                  defaultValue: "更换终端",
                }),
                onClick: onOpenTerminalSettings,
              },
            }
          : undefined,
      );
    }
  };

  const openReader = (session: SessionMeta) => {
    setReaderKey(getSessionKey(session));
    focusSoon(["session-reader-back"]);
  };

  const backToList = () => {
    const key = readerKey;
    setReaderKey(null);
    // 阅读页里翻过上一个 / 下一个时，原来那行可能已经滚出虚拟列表：先滚回去再把焦点放回去
    window.setTimeout(() => {
      const index = key
        ? listRows.findIndex((row) => row.kind === "session" && row.key === key)
        : -1;
      if (index >= 0) listVirtualizer.scrollToIndex(index, { align: "auto" });
      requestAnimationFrame(() =>
        focusSoon([key ? openButtonId(key) : "", "session-search"]),
      );
    }, 0);
  };

  const enterSelection = (session?: SessionMeta) => {
    setSelectionMode(true);
    setSelectedKeys(
      session && canDeleteSession(session)
        ? new Set([getSessionKey(session)])
        : new Set(),
    );
    setReaderKey(null);
  };

  const exitSelection = () => {
    setSelectionMode(false);
    setSelectedKeys(new Set());
  };

  const toggleSelected = (session: SessionMeta, checked: boolean) => {
    if (!canDeleteSession(session)) return;
    const key = getSessionKey(session);
    setSelectedKeys((current) => {
      const next = new Set(current);
      if (checked) next.add(key);
      else next.delete(key);
      return next;
    });
  };

  const selectedSessions = useMemo(
    () =>
      sessions.filter(
        (session) =>
          selectedKeys.has(getSessionKey(session)) && canDeleteSession(session),
      ),
    [sessions, selectedKeys],
  );
  const matchKeys = useMemo(
    () => new Set(matches.map(getSessionKey)),
    [matches],
  );
  const hiddenSelected = selectedSessions.filter(
    (session) => !matchKeys.has(getSessionKey(session)),
  ).length;

  const openDelete = (targets: SessionMeta[], source: DeleteSource) => {
    const deletable = targets.filter(canDeleteSession);
    if (deletable.length === 0) return;
    deleteSourceRef.current = source;
    setDeleteTargets(deletable);
  };

  const cancelDelete = () => {
    if (isDeleting) return;
    setDeleteTargets(null);
  };

  const handleDeleteConfirm = async () => {
    if (!deleteTargets || deleteTargets.length === 0 || isDeleting) return;
    const targets = deleteTargets.filter(canDeleteSession);
    const source = deleteSourceRef.current;
    setDeleteTargets(null);
    if (targets.length === 0) return;

    if (targets.length === 1) {
      const [target] = targets;
      try {
        await deleteSessionMutation.mutateAsync({
          providerId: target.providerId,
          sessionId: target.sessionId,
          sourcePath: target.sourcePath!,
        });
      } catch {
        // 失败的 toast 由 mutation 统一给
        return;
      }
      setSelectedKeys((current) => {
        const next = new Set(current);
        next.delete(getSessionKey(target));
        return next;
      });
      if (source === "bar") exitSelection();
      if (readerKey === getSessionKey(target)) {
        setReaderKey(null);
        focusSoon(["session-search"]);
      }
      return;
    }

    setIsBatchDeleting(true);
    try {
      const results = await sessionsApi.deleteMany(
        targets.map((session) => ({
          providerId: session.providerId,
          sessionId: session.sessionId,
          sourcePath: session.sourcePath!,
        })),
      );

      const deletedKeys = results
        .filter((result) => result.success)
        .map(
          (result) =>
            `${result.providerId}:${result.sessionId}:${result.sourcePath ?? ""}`,
        );
      const failedErrors = results
        .filter((result) => !result.success)
        .map((result) => result.error || t("common.unknown"));

      if (deletedKeys.length > 0) {
        const deletedKeySet = new Set(deletedKeys);
        queryClient.setQueryData<SessionMeta[]>(["sessions"], (current) =>
          (current ?? []).filter(
            (session) => !deletedKeySet.has(getSessionKey(session)),
          ),
        );
      }
      results
        .filter((result) => result.success)
        .forEach((result) => {
          queryClient.removeQueries({
            queryKey: sessionKeys.messages(
              result.providerId,
              result.sourcePath,
            ),
          });
          queryClient.removeQueries({
            queryKey: sessionKeys.transcript(
              result.providerId,
              result.sourcePath,
            ),
          });
        });

      setSelectedKeys((current) => {
        const next = new Set(current);
        deletedKeys.forEach((key) => next.delete(key));
        return next;
      });

      await queryClient.invalidateQueries({ queryKey: ["sessions"] });

      if (deletedKeys.length > 0) {
        toast.success(
          t("sessionManager.batchDeleteSuccess", {
            defaultValue: "已删除 {{count}} 个会话",
            count: deletedKeys.length,
          }),
        );
      }
      if (failedErrors.length > 0) {
        toast.error(
          t("sessionManager.batchDeleteFailed", {
            defaultValue: "{{failed}} 个会话删除失败",
            failed: failedErrors.length,
          }),
          { description: failedErrors[0] },
        );
      } else if (source === "bar") {
        exitSelection();
        focusSoon(["session-search"]);
      }
    } catch (error) {
      toast.error(
        extractErrorMessage(error) ||
          t("sessionManager.batchDeleteRequestFailed", {
            defaultValue: "批量删除失败，请稍后重试",
          }),
      );
    } finally {
      setIsBatchDeleting(false);
    }
  };

  const toggleGroup = (key: string) => {
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const refreshList = async () => {
    await refetch();
    toast.success(
      t("sessionManager.listRefreshed", { defaultValue: "已刷新会话列表" }),
    );
  };

  const reloadMessages = async () => {
    const result = (await transcript.refetch()) as { error?: unknown };
    if (!result.error) {
      toast.success(
        t("sessionManager.reloaded", { defaultValue: "已重新读取这个会话" }),
      );
    }
  };

  const handleRootKeyDown = (event: React.KeyboardEvent) => {
    if (event.key !== "Escape" || event.defaultPrevented) return;
    const target = event.target as HTMLElement | null;
    if (
      target?.closest(
        '[role="menu"],[role="dialog"],[role="alertdialog"],[role="search"]',
      )
    ) {
      return;
    }
    if (readerSession) {
      backToList();
    } else if (selectionMode) {
      exitSelection();
    }
  };

  const appName = (app: string) =>
    isSessionAppId(app) ? APP_DISPLAY_NAME[app] : app;
  const showAppIcon = appFilter === "all";
  const totalCount = scopedSessions.length;
  const currentCount =
    appFilter === "all" ? totalCount : (counts.get(appFilter) ?? 0);
  const noSessionsLabel = t("sessionManager.noSessionsCount", {
    defaultValue: "无会话",
  });

  // ─── 列表行 ────────────────────────────────────────────────────────────
  const renderRow = (session: SessionMeta, bordered: boolean) => {
    const key = getSessionKey(session);
    return (
      <SessionItem
        key={key}
        session={session}
        showAppIcon={showAppIcon}
        showDir={groupMode === "time"}
        selectionMode={selectionMode}
        isChecked={selectedKeys.has(key)}
        searchQuery={trimmedQuery}
        launchTerminal={terminalName}
        bordered={bordered}
        openButtonId={openButtonId(key)}
        onOpen={() => openReader(session)}
        onToggleChecked={(checked) => toggleSelected(session, checked)}
        onStartSelect={() => enterSelection(session)}
        onLaunch={() => void handleLaunch(session)}
        onCopyResume={() =>
          session.resumeCommand &&
          handleCopy(
            session.resumeCommand,
            terminalName
              ? t("sessionManager.resumeCommandCopied", {
                  defaultValue: "已复制恢复命令",
                })
              : t("sessionManager.resumeCopiedNoLaunch", {
                  defaultValue:
                    "已复制恢复命令。这个平台暂不支持一键恢复，请在项目目录下粘贴到终端运行",
                }),
          )
        }
        onCopyId={() =>
          handleCopy(
            session.sessionId,
            t("sessionManager.sessionIdCopied", {
              defaultValue: "已复制会话 ID",
            }),
          )
        }
        onCopySource={() =>
          session.sourcePath &&
          handleCopy(
            session.sourcePath,
            t("sessionManager.sourcePathCopied", {
              defaultValue: "已复制源文件路径",
            }),
          )
        }
        onDelete={() => openDelete([session], "row")}
      />
    );
  };

  const bucketLabel = (bucket: SessionTimeBucket) =>
    ({
      today: t("sessionManager.bucketToday", { defaultValue: "今天" }),
      yesterday: t("sessionManager.bucketYesterday", { defaultValue: "昨天" }),
      thisWeek: t("sessionManager.bucketThisWeek", { defaultValue: "本周" }),
      earlier: t("sessionManager.bucketEarlier", { defaultValue: "更早" }),
    })[bucket];

  // 列表虚拟化：几千个会话也只渲染可视区域和前后几行。分组标题和会话行拍平成一列，
  // 项目分组的标题用 rangeExtractor 留在渲染范围里并吸顶（原来的 sticky 效果）。
  const listRows = useMemo<SessionListRow[]>(() => {
    const rows: SessionListRow[] = [];
    if (groupMode === "time") {
      timeGroups.forEach((group, groupIndex) => {
        rows.push({
          kind: "time",
          key: `time:${group.bucket}`,
          bucket: group.bucket,
          first: groupIndex === 0,
        });
        group.sessions.forEach((session, index) =>
          rows.push({
            kind: "session",
            key: getSessionKey(session),
            session,
            bordered: index > 0,
          }),
        );
      });
    } else {
      projectGroups.forEach((group, groupIndex) => {
        rows.push({
          kind: "project",
          key: `project:${group.key}`,
          group,
          first: groupIndex === 0,
        });
        if (!isGroupOpen(group.key)) return;
        group.sessions.forEach((session) =>
          rows.push({
            kind: "session",
            key: getSessionKey(session),
            session,
            bordered: true,
          }),
        );
      });
    }
    return rows;
  }, [groupMode, timeGroups, projectGroups, isGroupOpen]);

  const stickyIndexes = useMemo(
    () =>
      listRows.flatMap((row, index) => (row.kind === "project" ? [index] : [])),
    [listRows],
  );
  const activeStickyRef = useRef<number | null>(null);
  const rangeExtractor = useCallback(
    (range: Range) => {
      const active = [...stickyIndexes]
        .reverse()
        .find((index) => range.startIndex >= index);
      activeStickyRef.current = active ?? null;
      const next = new Set(defaultRangeExtractor(range));
      if (active !== undefined) next.add(active);
      return [...next].sort((x, y) => x - y);
    },
    [stickyIndexes],
  );
  const listScrollRef = useRef<HTMLDivElement>(null);
  const listVirtualizer = useVirtualizer({
    count: listRows.length,
    getScrollElement: () => listScrollRef.current,
    estimateSize: (index) =>
      LIST_ROW_HEIGHT[listRows[index]?.kind ?? "session"],
    getItemKey: (index) => listRows[index]?.key ?? index,
    overscan: 10,
    rangeExtractor,
    // 阅读页打开时列表是 display:none，ResizeObserver 会报 0×0。照收的话虚拟列表把行全卸掉，
    // 返回时先画一帧空列表、等下一次测量才回来；忽略这种尺寸，原来的行一直留着
    observeElementRect: (instance, cb) =>
      observeElementRect(instance, (rect) => {
        if (rect.width === 0 && rect.height === 0) return;
        cb(rect);
      }),
  });

  const renderProjectHeader = (
    group: (typeof projectGroups)[number],
    first: boolean,
  ) => {
    const open = isGroupOpen(group.key);
    const fullPath = group.projectDir ? shortenHomePath(group.projectDir) : "";
    const pathHead = fullPath.endsWith(group.label)
      ? fullPath.slice(0, fullPath.length - group.label.length)
      : fullPath;
    const pathTail = fullPath.endsWith(group.label) ? group.label : "";
    let meta: string;
    if (appFilter === "all") {
      const perApp = new Map<string, number>();
      group.sessions.forEach((session) =>
        perApp.set(
          session.providerId,
          (perApp.get(session.providerId) ?? 0) + 1,
        ),
      );
      meta = Array.from(perApp.entries())
        .sort((a, b) => b[1] - a[1])
        .map(([app, count]) => `${appName(app)} ${count}`)
        .join(" · ");
    } else {
      meta = t("sessionManager.groupMeta", {
        defaultValue: "{{count}} 个会话 · {{time}}",
        count: group.sessions.length,
        time: formatRelativeTime(group.latest, t),
      });
    }
    return (
      <div
        className={cn(
          "flex h-9 items-center gap-3 bg-subtle pe-3 ps-2.5",
          !first && "border-t border-border",
        )}
      >
        <h3 className="m-0 flex min-w-0 flex-1 text-body font-semibold text-fg-1">
          <button
            type="button"
            aria-expanded={open}
            onClick={() => toggleGroup(group.key)}
            title={group.projectDir ?? undefined}
            className="flex h-7 min-w-0 max-w-full items-center gap-2.5 rounded-control pe-2 ps-1.5 text-left font-semibold transition-colors hover:bg-selected"
          >
            <ChevronDown
              aria-hidden="true"
              strokeWidth={2}
              className={cn(
                "h-4 w-4 shrink-0 text-fg-2 transition-transform duration-150",
                !open && "-rotate-90",
              )}
            />
            <span className="shrink-0 whitespace-nowrap">{group.label}</span>
            {fullPath && (
              <span className="flex min-w-0 whitespace-nowrap text-caption font-normal text-fg-3">
                <span className="min-w-0 truncate">{pathHead}</span>
                <span className="shrink-0">{pathTail}</span>
              </span>
            )}
          </button>
        </h3>
        <span
          className="min-w-0 max-w-[55%] truncate whitespace-nowrap text-caption tabular-nums text-fg-2"
          title={meta}
        >
          {meta}
        </span>
      </div>
    );
  };

  const renderList = () => (
    <div
      className="relative w-full"
      style={{ height: listVirtualizer.getTotalSize() }}
    >
      {listVirtualizer.getVirtualItems().map((item) => {
        const row = listRows[item.index];
        if (!row) return null;
        const sticky = activeStickyRef.current === item.index;
        return (
          <div
            key={item.key}
            className={cn(
              "inset-x-0 top-0",
              sticky ? "sticky z-[2]" : "absolute",
            )}
            style={
              sticky ? undefined : { transform: `translateY(${item.start}px)` }
            }
          >
            {row.kind === "time" ? (
              <h3
                className={cn(
                  "m-0 flex h-7 items-end px-4 pb-0.5 text-caption font-semibold text-fg-2",
                  !row.first && "border-t border-border",
                )}
              >
                {bucketLabel(row.bucket)}
              </h3>
            ) : row.kind === "project" ? (
              renderProjectHeader(row.group, row.first)
            ) : (
              renderRow(row.session, row.bordered)
            )}
          </div>
        );
      })}
    </div>
  );

  // ─── 空状态 ────────────────────────────────────────────────────────────
  const renderEmpty = () => {
    if (trimmedQuery) {
      return (
        <EmptyState
          title={t("sessionManager.emptySearch", {
            defaultValue: "没有标题、目录或首末消息匹配“{{query}}”的会话",
            query: trimmedQuery,
          })}
          action={t("sessionManager.clearSearchAction", {
            defaultValue: "清除搜索",
          })}
          onAction={() => {
            setQuery("");
            focusSoon(["session-search"]);
          }}
        />
      );
    }
    return (
      <EmptyState
        title={
          appFilter === "all"
            ? t("sessionManager.emptyAll", {
                defaultValue: "还没有会话记录",
              })
            : t("sessionManager.emptyApp", {
                defaultValue: "还没有 {{app}} 的会话记录",
                app: appName(appFilter),
              })
        }
        hint={
          appFilter === "all"
            ? undefined
            : t("sessionManager.emptyAppHint", {
                defaultValue: "CC Switch 读取 {{path}} 下的会话数据",
                path: SESSION_SOURCE_PATHS[appFilter][0],
              })
        }
        action={t("common.refresh", { defaultValue: "刷新" })}
        onAction={() => void refreshList()}
      />
    );
  };

  const piStatus = piSessionDiscovery.data?.status;

  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      onKeyDown={handleRootKeyDown}
      onWheel={(event) => event.stopPropagation()}
    >
      {/* 阅读页自己画页头（返回、会话名和会话的操作并在一条里），这里只在列表时画 */}
      {!readerSession && (
        <AppPageHeader
          icon={<History className="h-5 w-5" strokeWidth={1.5} />}
          title={t("nav.sessions", { defaultValue: "会话" })}
          titleExtra={
            <HelpTip
              title={t("sessionManager.helpTitle", {
                defaultValue: "会话从哪里来",
              })}
            >
              {t("sessionManager.helpBody", {
                defaultValue:
                  "CC Switch 读取各个应用保存在本机的会话记录，位置见右上角 ⋯ 里的「会话记录在哪里」。Claude Desktop 没有单独的会话记录。",
              })}
            </HelpTip>
          }
          actions={
            <DropdownMenu>
              <HoverTip content={t("common.more", { defaultValue: "更多" })}>
                <DropdownMenuTrigger asChild>
                  <Button
                    variant="quiet"
                    size="icon-compact"
                    className="h-8 w-8"
                    aria-label={t("sessionManager.moreActions", {
                      defaultValue: "会话的更多操作",
                    })}
                  >
                    <MoreHorizontal className="h-4 w-4" />
                  </Button>
                </DropdownMenuTrigger>
              </HoverTip>
              <DropdownMenuContent
                align="end"
                className="w-[196px] rounded-panel p-1 shadow-v7-md"
              >
                <DropdownMenuItem
                  className={sessionMenuItemClass}
                  onSelect={() => void refreshList()}
                >
                  {t("sessionManager.refreshList", {
                    defaultValue: "刷新列表",
                  })}
                </DropdownMenuItem>
                <DropdownMenuItem
                  className={sessionMenuItemClass}
                  onSelect={() => {
                    enterSelection();
                    focusSoon(["session-search"]);
                  }}
                >
                  {t("sessionManager.selectMany", {
                    defaultValue: "选择多个…",
                  })}
                </DropdownMenuItem>
                {onOpenTerminalSettings && (
                  <DropdownMenuItem
                    className={sessionMenuItemClass}
                    onSelect={onOpenTerminalSettings}
                  >
                    {t("sessionManager.preferredTerminal", {
                      defaultValue: "首选终端…",
                    })}
                  </DropdownMenuItem>
                )}
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  className={sessionMenuItemClass}
                  onSelect={() => setWhereOpen(true)}
                >
                  {t("sessionManager.whereMenu", {
                    defaultValue: "会话记录在哪里…",
                  })}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          }
        />
      )}

      <div id="main-content" className="flex min-h-0 flex-1 flex-col">
        {readerSession && (
          <SessionReader
            key={readerKey}
            session={readerSession}
            appName={appName(readerSession.providerId)}
            transcript={transcript}
            listQuery={trimmedQuery}
            launchTerminal={terminalName}
            hasPrev={readerIndex > 0}
            hasNext={
              readerIndex >= 0 && readerIndex < orderedSessions.length - 1
            }
            onPrev={() =>
              readerIndex > 0 &&
              setReaderKey(getSessionKey(orderedSessions[readerIndex - 1]))
            }
            onNext={() =>
              readerIndex >= 0 &&
              readerIndex < orderedSessions.length - 1 &&
              setReaderKey(getSessionKey(orderedSessions[readerIndex + 1]))
            }
            onBack={backToList}
            onLaunch={() => void handleLaunch(readerSession)}
            onCopy={handleCopy}
            onOpenTerminalSettings={onOpenTerminalSettings ?? (() => undefined)}
            onReload={() => void reloadMessages()}
            onDelete={() => openDelete([readerSession], "reader")}
          />
        )}

        {/* 列表在阅读页打开时只是藏起来，返回后滚动位置还在 */}
        <div
          hidden={Boolean(readerSession)}
          className={cn(
            "flex min-h-0 flex-1 flex-col",
            readerSession && "hidden",
          )}
        >
          <div className="flex shrink-0 items-center gap-2 px-6 pt-3.5">
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  variant="neutral"
                  size="regular"
                  className="shrink-0 gap-2 pe-2 ps-2.5"
                >
                  <span className="sr-only">
                    {t("sessionManager.appPrefix", { defaultValue: "应用：" })}
                  </span>
                  {appFilter === "all" ? (
                    <LayoutGrid
                      aria-hidden="true"
                      className="h-4 w-4 text-fg-2"
                      strokeWidth={2}
                    />
                  ) : (
                    <AppGlyph
                      app={appFilter}
                      size={16}
                      badgeClassName="bg-surface"
                    />
                  )}
                  <span>
                    {appFilter === "all"
                      ? t("sessionManager.allApps", {
                          defaultValue: "全部应用",
                        })
                      : appName(appFilter)}
                  </span>
                  <span className="text-caption font-normal tabular-nums text-fg-2">
                    {currentCount || appFilter === "all"
                      ? currentCount
                      : noSessionsLabel}
                  </span>
                  <ChevronDown
                    aria-hidden="true"
                    className="h-3.5 w-3.5 text-fg-2"
                    strokeWidth={2}
                  />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent
                align="start"
                aria-label={t("sessionManager.chooseApp", {
                  defaultValue: "选择应用",
                })}
                className="w-[240px] rounded-panel p-1 shadow-v7-md"
              >
                <AppMenuItem
                  pressed={appFilter === "all"}
                  icon={
                    <LayoutGrid
                      className="h-4 w-4 text-fg-2"
                      strokeWidth={1.5}
                    />
                  }
                  label={t("sessionManager.allApps", {
                    defaultValue: "全部应用",
                  })}
                  count={String(totalCount)}
                  onSelect={() => setAppFilter("all")}
                />
                <DropdownMenuSeparator />
                {menuApps.map((app) => {
                  const count = counts.get(app) ?? 0;
                  return (
                    <AppMenuItem
                      key={app}
                      pressed={appFilter === app}
                      dimmed={count === 0}
                      icon={
                        <AppGlyph
                          app={app}
                          size={16}
                          badgeClassName="bg-popover"
                          className={count === 0 ? "opacity-60" : undefined}
                        />
                      }
                      label={APP_DISPLAY_NAME[app]}
                      count={count ? String(count) : noSessionsLabel}
                      onSelect={() => setAppFilter(app)}
                    />
                  );
                })}
              </DropdownMenuContent>
            </DropdownMenu>

            <div className="relative min-w-0 flex-1">
              <SearchField
                id="session-search"
                ref={searchRef}
                value={query}
                onValueChange={setQuery}
                clearLabel={t("sessionManager.clearSearch", {
                  defaultValue: "清空搜索",
                })}
                aria-label={t("sessionManager.searchSessions", {
                  defaultValue: "搜索会话",
                })}
                placeholder={t("sessionManager.searchPlaceholder", {
                  defaultValue: "搜索标题、目录、首末消息或会话 ID",
                })}
                autoComplete="off"
              />
              <span role="status" className="sr-only">
                {trimmedQuery
                  ? t("sessionManager.foundCount", {
                      defaultValue: "找到 {{count}} 个会话",
                      count: matches.length,
                    })
                  : ""}
              </span>
            </div>

            <SegmentedControl<GroupMode>
              aria-label={t("sessionManager.groupBy", {
                defaultValue: "分组方式",
              })}
              value={groupMode}
              onValueChange={setGroupMode}
              className="h-8 shrink-0 rounded-[8px] [&>button]:rounded-[5px] [&>button]:px-3"
              items={[
                {
                  value: "time",
                  label: t("sessionManager.groupByTime", {
                    defaultValue: "按时间",
                  }),
                },
                {
                  value: "project",
                  label: t("sessionManager.groupByProject", {
                    defaultValue: "按项目",
                  }),
                },
              ]}
            />
          </div>

          {fromApp === "claude-desktop" && appFilter === "claude" && (
            <p className="m-0 mx-6 mt-2 shrink-0 text-caption text-fg-2">
              {t("sessionManager.desktopNote", {
                defaultValue:
                  "Claude Desktop 没有单独的会话记录，这里显示 Claude Code 的会话。",
              })}
            </p>
          )}

          {appFilter === "pi" && piStatus === "requires_project_context" && (
            <div role="status" className="mx-6 mt-3 shrink-0">
              <Notice
                tone="warning"
                title={
                  <>
                    {t("sessionManager.piRelativeSessionDir")}{" "}
                    <code className="font-mono text-caption">
                      {piSessionDiscovery.data?.status ===
                      "requires_project_context"
                        ? piSessionDiscovery.data.configuredPath
                        : ""}
                    </code>
                  </>
                }
              />
            </div>
          )}
          {appFilter === "pi" &&
            (piStatus === "unavailable" || piSessionDiscovery.isError) && (
              <div role="alert" className="mx-6 mt-3 shrink-0">
                <Notice
                  tone="danger"
                  title={t("sessionManager.piDiscoveryUnavailable", {
                    error:
                      piSessionDiscovery.data?.status === "unavailable"
                        ? piSessionDiscovery.data.reason
                        : extractErrorMessage(piSessionDiscovery.error),
                  })}
                />
              </div>
            )}

          {selectionMode && (
            <div className="flex shrink-0 items-center gap-1.5 whitespace-nowrap px-6 pt-2 text-caption text-fg-2">
              <span className="tabular-nums">
                {trimmedQuery
                  ? t("sessionManager.matchCount", {
                      defaultValue: "匹配 {{count}} 个会话",
                      count: matches.length,
                    })
                  : t("sessionManager.totalCount", {
                      defaultValue: "共 {{count}} 个会话",
                      count: matches.length,
                    })}
              </span>
              <span aria-hidden="true">·</span>
              <button
                type="button"
                className={linkButton}
                onClick={() =>
                  setSelectedKeys((current) => {
                    const next = new Set(current);
                    matches
                      .filter(canDeleteSession)
                      .forEach((session) => next.add(getSessionKey(session)));
                    return next;
                  })
                }
              >
                {t("sessionManager.selectAllMatches", {
                  defaultValue: "全选匹配结果",
                })}
              </button>
              <span aria-hidden="true">·</span>
              <button
                type="button"
                className={linkButton}
                onClick={() => setSelectedKeys(new Set())}
              >
                {t("sessionManager.clearSelectionShort", {
                  defaultValue: "清空",
                })}
              </button>
              <span aria-hidden="true">·</span>
              <button
                type="button"
                className={linkButton}
                onClick={exitSelection}
              >
                {t("sessionManager.exitSelection", {
                  defaultValue: "退出选择",
                })}
              </button>
            </div>
          )}

          {isLoading ? (
            <div className="flex flex-1 items-center justify-center text-body text-fg-2">
              {t("sessionManager.loadingSessions", {
                defaultValue: "加载会话中...",
              })}
            </div>
          ) : matches.length === 0 ? (
            renderEmpty()
          ) : (
            <>
              <div
                role="region"
                aria-label={t("sessionManager.sessionList", {
                  defaultValue: "会话列表",
                })}
                ref={listScrollRef}
                className="mx-6 mt-3 min-h-0 shrink overflow-y-auto scroll-stable overscroll-contain rounded-panel border border-border bg-surface"
                style={{ scrollPaddingTop: groupMode === "project" ? 44 : 8 }}
              >
                {renderList()}
              </div>
              {appFilter === "hermes" && (
                <p className="m-0 mx-6 mt-2 shrink-0 text-caption text-fg-3">
                  {t("sessionManager.hermesLimit", {
                    defaultValue: "Hermes 只显示最近 500 个会话",
                  })}
                </p>
              )}
              <div className="flex-1" />
            </>
          )}
          <div className="h-4 shrink-0" />

          {selectionMode && (
            <div className="flex h-14 shrink-0 items-center gap-2 border-t border-border bg-surface pe-4 ps-6">
              <span
                role="status"
                className="min-w-0 flex-1 truncate text-body text-fg-1"
              >
                <span className="font-medium">
                  {selectedSessions.length
                    ? t("sessionManager.selectedSessions", {
                        defaultValue: "已选 {{count}} 个会话",
                        count: selectedSessions.length,
                      })
                    : t("sessionManager.noneSelected", {
                        defaultValue: "还没有选择会话",
                      })}
                </span>
                {hiddenSelected > 0 && (
                  <span className="text-caption text-fg-2">
                    {t("sessionManager.hiddenSelected", {
                      defaultValue: " · 其中 {{count}} 个不在当前结果里",
                      count: hiddenSelected,
                    })}
                  </span>
                )}
              </span>
              <Button variant="neutral" size="regular" onClick={exitSelection}>
                {t("common.cancel", { defaultValue: "取消" })}
              </Button>
              <Button
                variant="solid"
                size="regular"
                disabled={isDeleting || selectedSessions.length === 0}
                onClick={() => openDelete(selectedSessions, "bar")}
              >
                {isBatchDeleting
                  ? t("sessionManager.batchDeleting", {
                      defaultValue: "删除中...",
                    })
                  : selectedSessions.length
                    ? t("sessionManager.deleteCountEllipsis", {
                        defaultValue: "删除 {{count}} 个会话…",
                        count: selectedSessions.length,
                      })
                    : t("sessionManager.deleteSessionEllipsis", {
                        defaultValue: "删除会话…",
                      })}
              </Button>
            </div>
          )}
        </div>
      </div>

      <SessionDeleteDialog
        targets={deleteTargets}
        pending={isDeleting}
        onConfirm={() => void handleDeleteConfirm()}
        onCancel={cancelDelete}
      />
      <SessionSourcesDialog
        open={whereOpen}
        onClose={() => setWhereOpen(false)}
      />
    </div>
  );
}

function AppMenuItem({
  pressed,
  dimmed = false,
  icon,
  label,
  count,
  onSelect,
}: {
  pressed: boolean;
  dimmed?: boolean;
  icon: React.ReactNode;
  label: string;
  count: string;
  onSelect: () => void;
}) {
  return (
    <DropdownMenuItem
      role="menuitemradio"
      aria-checked={pressed}
      onSelect={onSelect}
      className={cn(
        sessionMenuItemClass,
        "gap-2 pe-2",
        pressed && "font-medium",
        dimmed && "text-fg-3",
      )}
    >
      {icon}
      <span className="min-w-0 flex-1 truncate">{label}</span>
      <span
        className={cn(
          "text-caption font-normal tabular-nums",
          dimmed ? "text-fg-3" : "text-fg-2",
        )}
      >
        {count}
      </span>
      <Check
        aria-hidden="true"
        strokeWidth={1.5}
        className={cn("h-3.5 w-3.5 shrink-0", !pressed && "invisible")}
      />
    </DropdownMenuItem>
  );
}

function EmptyState({
  title,
  hint,
  action,
  onAction,
}: {
  title: string;
  hint?: string;
  action: string;
  onAction: () => void;
}) {
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-1.5 px-6 pb-10 text-center">
      <p className="m-0 max-w-[560px] text-section text-fg-1">{title}</p>
      {hint && <p className="m-0 text-caption text-fg-2">{hint}</p>}
      <Button
        variant="neutral"
        size="regular"
        className="mt-2.5"
        onClick={onAction}
      >
        {action}
      </Button>
    </div>
  );
}
