import { invoke } from "@tauri-apps/api/core";
import type {
  CreateTaskInput,
  TaskGateway,
  TaskLedger,
  TaskLedgerItem,
} from "../types";

export const tauriTaskGateway: TaskGateway = {
  listLedger: () => invoke<TaskLedger>("list_tandem_ledger"),
  createTask: (input: CreateTaskInput) =>
    invoke<TaskLedgerItem>("create_tandem_task", { input }),
  confirmCompleted: (taskId: string) =>
    invoke<TaskLedgerItem>("confirm_tandem_task_completed", { taskId }),
};
