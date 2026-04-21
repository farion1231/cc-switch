import { useEffect, useMemo, useState } from "react";
import {
  ArrowDown,
  ArrowUp,
  Copy,
  FolderOpen,
  Layers3,
  Plus,
  Trash2,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import type { AppId } from "@/lib/api";
import type { Provider } from "@/types";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

export interface BatchTerminalLaunchOptions {
  bypass: boolean;
  enableTelegramChannel: boolean;
}

export interface BatchTerminalLaunchTask {
  providerId: string;
  directories: string[];
}

interface DraftTask {
  id: string;
  providerId: string;
  count: number;
  directories: Array<string | null>;
}

interface BatchTerminalLaunchDialogProps {
  isOpen: boolean;
  app: AppId;
  providers: Record<string, Provider>;
  onConfirm: (
    options: BatchTerminalLaunchOptions,
    tasks: BatchTerminalLaunchTask[],
  ) => void | Promise<void>;
  onPickDirectory: (context: {
    taskIndex: number;
    paneIndex: number;
    provider: Provider;
  }) => Promise<string | null>;
  onCancel: () => void;
}

const MAX_PANES_PER_TASK = 8;

const createTaskId = () =>
  `batch-task-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;

const normalizeCount = (value: number) => {
  if (!Number.isFinite(value)) return 1;
  return Math.min(MAX_PANES_PER_TASK, Math.max(1, Math.trunc(value)));
};

const syncDirectories = (
  directories: Array<string | null>,
  count: number,
): Array<string | null> => {
  const next = directories.slice(0, count);
  while (next.length < count) {
    next.push(null);
  }
  return next;
};

const buildInitialTasks = (providerIds: string[]): DraftTask[] => [
  {
    id: createTaskId(),
    providerId: providerIds[0] ?? "",
    count: 1,
    directories: [null],
  },
];

export function BatchTerminalLaunchDialog({
  isOpen,
  app,
  providers,
  onConfirm,
  onPickDirectory,
  onCancel,
}: BatchTerminalLaunchDialogProps) {
  const { t } = useTranslation();
  const providerList = useMemo(
    () =>
      Object.values(providers).sort(
        (a, b) => (a.sortIndex ?? 0) - (b.sortIndex ?? 0),
      ),
    [providers],
  );
  const providerIds = useMemo(() => providerList.map((p) => p.id), [providerList]);

  const [tasks, setTasks] = useState<DraftTask[]>(() =>
    buildInitialTasks(providerIds),
  );
  const [bypass, setBypass] = useState(false);
  const [enableTelegramChannel, setEnableTelegramChannel] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);

  useEffect(() => {
    if (isOpen) {
      setTasks(buildInitialTasks(providerIds));
      setBypass(false);
      setEnableTelegramChannel(false);
      setError(null);
      setIsSubmitting(false);
    }
  }, [isOpen, app, providerIds]);

  const updateTask = (taskId: string, updates: Partial<DraftTask>) => {
    setTasks((current) =>
      current.map((task) => {
        if (task.id !== taskId) return task;
        const nextCount = updates.count
          ? normalizeCount(updates.count)
          : task.count;
        const directories =
          updates.directories ?? syncDirectories(task.directories, nextCount);
        return {
          ...task,
          ...updates,
          count: nextCount,
          directories: syncDirectories(directories, nextCount),
        };
      }),
    );
  };

  const addTask = () => {
    setTasks((current) => [...current, buildInitialTasks(providerIds)[0]]);
  };

  const duplicateTask = (task: DraftTask) => {
    setTasks((current) => [
      ...current,
      {
        ...task,
        id: createTaskId(),
        directories: [...task.directories],
      },
    ]);
  };

  const removeTask = (taskId: string) => {
    setTasks((current) =>
      current.length === 1
        ? current
        : current.filter((task) => task.id !== taskId),
    );
  };

  const moveTask = (taskId: string, direction: -1 | 1) => {
    setTasks((current) => {
      const index = current.findIndex((task) => task.id === taskId);
      const target = index + direction;
      if (index < 0 || target < 0 || target >= current.length) return current;
      const next = [...current];
      [next[index], next[target]] = [next[target], next[index]];
      return next;
    });
  };

  const setDirectory = (
    taskId: string,
    paneIndex: number,
    directory: string | null,
  ) => {
    setTasks((current) =>
      current.map((task) => {
        if (task.id !== taskId) return task;
        const directories = [...task.directories];
        directories[paneIndex] = directory;
        return { ...task, directories };
      }),
    );
  };

  const pickDirectoryForSlot = async (
    task: DraftTask,
    taskIndex: number,
    paneIndex: number,
  ) => {
    const provider = providers[task.providerId];
    if (!provider) return null;
    const directory = await onPickDirectory({
      taskIndex,
      paneIndex,
      provider,
    });
    if (directory) {
      setDirectory(task.id, paneIndex, directory);
    }
    return directory;
  };

  const handleSubmit = async () => {
    setError(null);
    if (providerList.length === 0) {
      setError(t("provider.batchTerminal.noProviders", "当前应用没有可启动的供应商。"));
      return;
    }
    if (tasks.some((task) => !providers[task.providerId])) {
      setError(t("provider.batchTerminal.providerRequired", "每个任务都需要选择供应商。"));
      return;
    }

    setIsSubmitting(true);
    const completedTasks: DraftTask[] = tasks.map((task) => ({
      ...task,
      directories: [...task.directories],
    }));

    try {
      for (const [taskIndex, task] of completedTasks.entries()) {
        for (let paneIndex = 0; paneIndex < task.directories.length; paneIndex++) {
          if (task.directories[paneIndex]) continue;
          const provider = providers[task.providerId];
          const directory = await onPickDirectory({
            taskIndex,
            paneIndex,
            provider,
          });
          if (!directory) {
            setError(
              t(
                "provider.batchTerminal.directoryPickCancelled",
                "已取消目录选择，批量启动未执行。",
              ),
            );
            return;
          }
          task.directories[paneIndex] = directory;
        }
      }

      const nextTasks = completedTasks.map((task) => ({
        ...task,
        directories: [...task.directories],
      }));
      setTasks(nextTasks);
      await onConfirm(
        {
          bypass,
          enableTelegramChannel: app === "claude" ? enableTelegramChannel : false,
        },
        nextTasks.map((task) => ({
          providerId: task.providerId,
          directories: task.directories.filter(
            (directory): directory is string => typeof directory === "string",
          ),
        })),
      );
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(open) => {
        if (!open) onCancel();
      }}
    >
      <DialogContent className="max-w-5xl" zIndex="alert">
        <DialogHeader className="space-y-3">
          <DialogTitle className="flex items-center gap-2 text-lg font-semibold">
            <Layers3 className="h-5 w-5 text-emerald-500" />
            {t("provider.batchTerminal.title", "批量启动终端")}
          </DialogTitle>
          <DialogDescription className="text-sm text-muted-foreground">
            {t(
              "provider.batchTerminal.description",
              "按任务顺序启动 tmux session。每个任务会成为一个 tmux window，目录槽位会成为该 window 内的 pane；启动后可直接用鼠标点击 pane 或 window 切换。",
            )}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <section className="rounded-xl border border-border bg-muted/20 p-4">
            <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
              <div>
                <h3 className="text-sm font-medium">
                  {t("provider.batchTerminal.globalOptions", "全局参数")}
                </h3>
                <p className="text-xs text-muted-foreground">
                  {t(
                    "provider.batchTerminal.globalOptionsHint",
                    "所有任务共享这组启动参数。",
                  )}
                </p>
              </div>
              <div className="flex flex-wrap items-center gap-4">
                <label className="flex items-center gap-2 text-sm">
                  <Checkbox
                    aria-label={t("provider.batchTerminal.bypass", "越权启动")}
                    checked={bypass}
                    onCheckedChange={(checked) => setBypass(checked === true)}
                  />
                  {t("provider.batchTerminal.bypass", "越权启动")}
                </label>
                {app === "claude" && (
                  <label className="flex items-center gap-2 text-sm">
                    <Checkbox
                      aria-label={t(
                        "provider.terminalLaunchTelegram",
                        "TG 通信",
                      )}
                      checked={enableTelegramChannel}
                      onCheckedChange={(checked) =>
                        setEnableTelegramChannel(checked === true)
                      }
                    />
                    {t("provider.terminalLaunchTelegram", "TG 通信")}
                  </label>
                )}
              </div>
            </div>
          </section>

          {error && (
            <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
              {error}
            </div>
          )}

          <div className="max-h-[52vh] space-y-3 overflow-y-auto pr-1">
            {tasks.map((task, taskIndex) => {
              const provider = providers[task.providerId];
              return (
                <section
                  key={task.id}
                  data-testid="batch-terminal-task"
                  className="rounded-xl border border-border bg-card p-4 shadow-sm"
                >
                  <div className="flex flex-col gap-4">
                    <div className="flex flex-col gap-3 lg:flex-row lg:items-end">
                      <div className="flex h-9 w-12 items-center justify-center rounded-lg bg-emerald-500/10 text-sm font-semibold text-emerald-600 dark:text-emerald-400">
                        #{taskIndex + 1}
                      </div>
                      <div className="min-w-[220px] flex-1 space-y-1.5">
                        <Label htmlFor={`provider-${task.id}`}>Provider</Label>
                        <select
                          id={`provider-${task.id}`}
                          aria-label="Provider"
                          value={task.providerId}
                          onChange={(event) =>
                            updateTask(task.id, {
                              providerId: event.target.value,
                            })
                          }
                          className={cn(
                            "flex h-9 w-full rounded-md border border-border-default bg-background px-3 py-1 text-sm",
                            "focus:outline-none focus:ring-2 focus:ring-blue-500/20",
                          )}
                        >
                          {providerList.map((item) => (
                            <option key={item.id} value={item.id}>
                              {item.name}
                            </option>
                          ))}
                        </select>
                      </div>
                      <div className="w-32 space-y-1.5">
                        <Label htmlFor={`count-${task.id}`}>
                          {t("provider.batchTerminal.count", "启动数量")}
                        </Label>
                        <Input
                          id={`count-${task.id}`}
                          aria-label={t(
                            "provider.batchTerminal.count",
                            "启动数量",
                          )}
                          type="number"
                          min={1}
                          max={MAX_PANES_PER_TASK}
                          value={task.count}
                          onChange={(event) =>
                            updateTask(task.id, {
                              count: normalizeCount(
                                Number(event.target.value),
                              ),
                            })
                          }
                        />
                      </div>
                      <div className="flex flex-wrap gap-1.5">
                        <Button
                          type="button"
                          size="sm"
                          variant="outline"
                          aria-label="上移"
                          disabled={taskIndex === 0}
                          onClick={() => moveTask(task.id, -1)}
                        >
                          <ArrowUp className="h-4 w-4" />
                        </Button>
                        <Button
                          type="button"
                          size="sm"
                          variant="outline"
                          aria-label="下移"
                          disabled={taskIndex === tasks.length - 1}
                          onClick={() => moveTask(task.id, 1)}
                        >
                          <ArrowDown className="h-4 w-4" />
                        </Button>
                        <Button
                          type="button"
                          size="sm"
                          variant="outline"
                          aria-label="复制"
                          onClick={() => duplicateTask(task)}
                        >
                          <Copy className="h-4 w-4" />
                        </Button>
                        <Button
                          type="button"
                          size="sm"
                          variant="outline"
                          aria-label="删除"
                          disabled={tasks.length === 1}
                          onClick={() => removeTask(task.id)}
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      </div>
                    </div>

                    <div className="grid gap-2 md:grid-cols-2">
                      {task.directories.map((directory, paneIndex) => (
                        <div
                          key={`${task.id}-${paneIndex}`}
                          className="flex items-center gap-2 rounded-lg border border-border/70 bg-muted/20 p-2"
                        >
                          <div className="min-w-14 text-xs font-medium text-muted-foreground">
                            Pane {paneIndex + 1}
                          </div>
                          <div className="min-w-0 flex-1 truncate text-sm">
                            {directory ??
                              t(
                                "provider.batchTerminal.directoryEmpty",
                                "未选择目录",
                              )}
                          </div>
                          <Button
                            type="button"
                            size="sm"
                            variant="ghost"
                            disabled={!provider}
                            onClick={() =>
                              void pickDirectoryForSlot(
                                task,
                                taskIndex,
                                paneIndex,
                              )
                            }
                          >
                            <FolderOpen className="mr-1 h-4 w-4" />
                            {t(
                              "provider.batchTerminal.pickDirectory",
                              "选择目录",
                            )}
                          </Button>
                          <Button
                            type="button"
                            size="sm"
                            variant="ghost"
                            disabled={!directory}
                            onClick={() => setDirectory(task.id, paneIndex, null)}
                          >
                            {t("common.clear", "清空")}
                          </Button>
                        </div>
                      ))}
                    </div>
                  </div>
                </section>
              );
            })}
          </div>
        </div>

        <DialogFooter className="flex flex-col gap-2 pt-2 sm:flex-row sm:items-center sm:justify-between">
          <Button
            type="button"
            variant="outline"
            onClick={addTask}
            disabled={providerList.length === 0}
          >
            <Plus className="mr-2 h-4 w-4" />
            {t("provider.batchTerminal.addTask", "新增任务")}
          </Button>
          <div className="flex gap-2">
            <Button type="button" variant="outline" onClick={onCancel}>
              {t("common.cancel", "取消")}
            </Button>
            <Button
              type="button"
              onClick={() => void handleSubmit()}
              disabled={isSubmitting || providerList.length === 0}
              className="bg-emerald-600 text-white hover:bg-emerald-700"
            >
              {isSubmitting
                ? t("provider.batchTerminal.starting", "启动中...")
                : t("provider.batchTerminal.start", "开始批量启动")}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
