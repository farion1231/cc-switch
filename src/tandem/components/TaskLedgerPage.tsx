import { useState } from "react";
import { Check } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  useConfirmTaskCompleted,
  useCreateTask,
  useTaskLedger,
} from "../hooks/useTaskLedger";
import type { CreateTaskInput, TaskLedgerItem } from "../types";
import { NewTaskDialog } from "./NewTaskDialog";
import { TaskSection } from "./TaskSection";

export function TaskLedgerPage() {
  const ledger = useTaskLedger();
  const createTask = useCreateTask();
  const completeTask = useConfirmTaskCompleted();
  const [confirming, setConfirming] = useState<TaskLedgerItem | null>(null);

  const create = async (input: CreateTaskInput) => {
    try {
      await createTask.mutateAsync(input);
    } catch {
      toast.error("任务创建失败，请稍后重试");
      throw new Error("task creation failed");
    }
  };

  const confirm = async () => {
    if (!confirming) return;
    try {
      await completeTask.mutateAsync(confirming.task.id);
      setConfirming(null);
    } catch {
      setConfirming(null);
      toast.error("无法确认任务完成，请稍后重试");
    }
  };

  if (ledger.isPending) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        正在加载任务…
      </div>
    );
  }

  if (ledger.isError || !ledger.data) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-red-600 dark:text-red-400">
        任务列表加载失败
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-12 shrink-0 items-center justify-end border-b border-border px-4 sm:px-6">
        <NewTaskDialog submitting={createTask.isPending} onCreate={create} />
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        <TaskSection
          title="需要你处理"
          items={ledger.data.needsAttention}
          onComplete={setConfirming}
        />
        <TaskSection
          title="待验收"
          items={ledger.data.awaitingAcceptance}
          acceptance
          onComplete={setConfirming}
        />
        <TaskSection
          title="正在推进"
          items={ledger.data.active}
          onComplete={setConfirming}
        />
        <TaskSection
          title="最近可继续"
          items={ledger.data.recentResumable}
          onComplete={setConfirming}
        />
      </div>
      <Dialog
        open={Boolean(confirming)}
        onOpenChange={(open) => !open && setConfirming(null)}
      >
        <DialogContent
          role="alertdialog"
          overlayClassName="backdrop-blur-none"
          className="sm:max-w-md"
        >
          <DialogHeader>
            <DialogTitle>确认任务完成</DialogTitle>
            <DialogDescription>
              此操作会将任务移出当前列表。请确认已完成：
            </DialogDescription>
          </DialogHeader>
          <p className="truncate text-sm font-medium text-foreground">
            {confirming?.task.title}
          </p>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirming(null)}>
              取消
            </Button>
            <Button
              onClick={() => void confirm()}
              disabled={completeTask.isPending}
            >
              <Check className="h-4 w-4" aria-hidden="true" />
              确认完成
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
