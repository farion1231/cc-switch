import { Check, MoreHorizontal } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { TaskLedgerItem, TaskStatus } from "../types";

const statusLabels: Record<TaskStatus, string> = {
  needs_attention: "需要处理",
  awaiting_acceptance: "待验收",
  active: "推进中",
  paused: "已暂停",
  completed: "已完成",
};

const formatUpdatedAt = (value: number) =>
  new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(value);

export function TaskSection({
  title,
  items,
  acceptance = false,
  onComplete,
}: {
  title: string;
  items: TaskLedgerItem[];
  acceptance?: boolean;
  onComplete: (item: TaskLedgerItem) => void;
}) {
  return (
    <section
      aria-label={title}
      className="border-b border-border last:border-b-0"
    >
      <div className="flex h-10 items-center justify-between px-4 sm:px-6">
        <h2 className="text-sm font-semibold text-foreground">
          {title} <span className="text-muted-foreground">{items.length}</span>
        </h2>
      </div>
      {items.length === 0 ? (
        <p className="border-t border-border px-4 py-4 text-sm text-muted-foreground sm:px-6">
          暂无任务
        </p>
      ) : (
        <div className="border-t border-border">
          {items.map((item) => (
            <div
              key={item.task.id}
              className="flex min-h-16 items-center gap-3 border-b border-border px-4 py-3 last:border-b-0 sm:px-6"
            >
              <div className="min-w-0 flex-1">
                <p className="truncate text-sm font-medium text-foreground">
                  {item.task.title}
                </p>
                <div className="mt-1 flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
                  <span className="max-w-48 truncate">{item.project.name}</span>
                  <span>{statusLabels[item.task.status]}</span>
                  <time dateTime={new Date(item.task.updatedAt).toISOString()}>
                    {formatUpdatedAt(item.task.updatedAt)}
                  </time>
                </div>
              </div>
              {acceptance ? (
                <Button
                  size="sm"
                  onClick={() => onComplete(item)}
                  aria-label={`确认完成 ${item.task.title}`}
                >
                  <Check className="h-4 w-4" aria-hidden="true" />
                  <span className="hidden sm:inline">确认完成</span>
                </Button>
              ) : (
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button
                      size="icon"
                      variant="ghost"
                      className="h-8 w-8"
                      aria-label={`${item.task.title} 操作`}
                      title="任务操作"
                    >
                      <MoreHorizontal className="h-4 w-4" aria-hidden="true" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end">
                    <DropdownMenuItem onSelect={() => onComplete(item)}>
                      <Check className="h-4 w-4" aria-hidden="true" />
                      确认完成
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              )}
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
