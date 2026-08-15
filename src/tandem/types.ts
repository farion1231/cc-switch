export type TaskStatus =
  | "active"
  | "needs_attention"
  | "awaiting_acceptance"
  | "paused"
  | "completed";

export interface Project {
  id: string;
  name: string;
  rootPath: string;
  createdAt: number;
  updatedAt: number;
}

export interface TandemTask {
  id: string;
  projectId: string;
  title: string;
  originalInstruction: string;
  status: TaskStatus;
  createdAt: number;
  updatedAt: number;
  completedAt: number | null;
}

export interface TaskLedgerItem {
  task: TandemTask;
  project: Project;
}

export interface TaskLedger {
  needsAttention: TaskLedgerItem[];
  awaitingAcceptance: TaskLedgerItem[];
  active: TaskLedgerItem[];
  recentResumable: TaskLedgerItem[];
}

export interface CreateTaskInput {
  projectName: string;
  projectRootPath: string;
  title: string;
  originalInstruction: string;
}

export interface TaskGateway {
  listLedger(): Promise<TaskLedger>;
  createTask(input: CreateTaskInput): Promise<TaskLedgerItem>;
  confirmCompleted(taskId: string): Promise<TaskLedgerItem>;
}
