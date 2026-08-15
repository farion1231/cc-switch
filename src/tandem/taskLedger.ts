import type { TaskLedger, TaskLedgerItem } from "./types";

const newestFirst = (left: TaskLedgerItem, right: TaskLedgerItem) =>
  right.task.updatedAt - left.task.updatedAt ||
  (left.task.id < right.task.id ? -1 : left.task.id > right.task.id ? 1 : 0);

export function buildTaskLedger(items: TaskLedgerItem[]): TaskLedger {
  const needsAttention: TaskLedgerItem[] = [];
  const awaitingAcceptance: TaskLedgerItem[] = [];
  const active: TaskLedgerItem[] = [];
  const paused: TaskLedgerItem[] = [];

  for (const item of items) {
    switch (item.task.status) {
      case "needs_attention":
        needsAttention.push(item);
        break;
      case "awaiting_acceptance":
        awaitingAcceptance.push(item);
        break;
      case "active":
        active.push(item);
        break;
      case "paused":
        paused.push(item);
        break;
      case "completed":
        break;
    }
  }

  return {
    needsAttention: needsAttention.sort(newestFirst),
    awaitingAcceptance: awaitingAcceptance.sort(newestFirst),
    active: active.sort(newestFirst),
    recentResumable: paused.sort(newestFirst).slice(0, 10),
  };
}
