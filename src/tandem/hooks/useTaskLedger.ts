import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTaskGateway } from "../api/TaskGatewayProvider";
import type { CreateTaskInput, TaskLedger, TaskLedgerItem } from "../types";

export const TASK_LEDGER_QUERY_KEY = ["tandem", "task-ledger"] as const;

const addCreatedTask = (
  ledger: TaskLedger | undefined,
  item: TaskLedgerItem,
): TaskLedger => ({
  needsAttention: ledger?.needsAttention ?? [],
  awaitingAcceptance: ledger?.awaitingAcceptance ?? [],
  active: [item, ...(ledger?.active ?? [])],
  recentResumable: ledger?.recentResumable ?? [],
});

const removeCompletedTask = (
  ledger: TaskLedger | undefined,
  taskId: string,
): TaskLedger | undefined =>
  ledger && {
    needsAttention: ledger.needsAttention.filter(
      ({ task }) => task.id !== taskId,
    ),
    awaitingAcceptance: ledger.awaitingAcceptance.filter(
      ({ task }) => task.id !== taskId,
    ),
    active: ledger.active.filter(({ task }) => task.id !== taskId),
    recentResumable: ledger.recentResumable.filter(
      ({ task }) => task.id !== taskId,
    ),
  };

export function useTaskLedger() {
  const gateway = useTaskGateway();
  return useQuery({
    queryKey: TASK_LEDGER_QUERY_KEY,
    queryFn: () => gateway.listLedger(),
    staleTime: Number.POSITIVE_INFINITY,
  });
}

export function useCreateTask() {
  const gateway = useTaskGateway();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input: CreateTaskInput) => gateway.createTask(input),
    onSuccess: (item) => {
      queryClient.setQueryData<TaskLedger>(TASK_LEDGER_QUERY_KEY, (ledger) =>
        addCreatedTask(ledger, item),
      );
    },
  });
}

export function useConfirmTaskCompleted() {
  const gateway = useTaskGateway();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (taskId: string) => gateway.confirmCompleted(taskId),
    onSuccess: (item) => {
      queryClient.setQueryData<TaskLedger>(TASK_LEDGER_QUERY_KEY, (ledger) =>
        removeCompletedTask(ledger, item.task.id),
      );
    },
  });
}
