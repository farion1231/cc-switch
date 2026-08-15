import { buildTaskLedger } from "../taskLedger";
import type {
  CreateTaskInput,
  TaskGateway,
  TaskLedgerItem,
  TaskStatus,
} from "../types";

const project = {
  id: "demo-project",
  name: "Tandem Demo",
  rootPath: "/demo/tandem",
  createdAt: 1_700_000_000_000,
  updatedAt: 1_700_000_000_000,
};

const demoItem = (
  id: string,
  title: string,
  status: TaskStatus,
  updatedAt: number,
): TaskLedgerItem => ({
  project: { ...project },
  task: {
    id,
    projectId: project.id,
    title,
    originalInstruction: `Demo instruction for ${title}`,
    status,
    createdAt: updatedAt - 60_000,
    updatedAt,
    completedAt: null,
  },
});

let items: TaskLedgerItem[] = [
  demoItem(
    "demo-attention",
    "Resolve foundation alert",
    "needs_attention",
    1_700_000_004_000,
  ),
  demoItem(
    "demo-acceptance",
    "Review foundation acceptance",
    "awaiting_acceptance",
    1_700_000_003_000,
  ),
  demoItem(
    "demo-active",
    "Continue foundation build",
    "active",
    1_700_000_002_000,
  ),
  demoItem(
    "demo-paused",
    "Resume foundation task",
    "paused",
    1_700_000_001_000,
  ),
];
let sequence = 1;

export const demoTaskGateway: TaskGateway = {
  async listLedger() {
    return buildTaskLedger(structuredClone(items));
  },
  async createTask(input: CreateTaskInput) {
    const now = 1_700_000_005_000 + sequence;
    const id = `demo-created-${sequence++}`;
    const item: TaskLedgerItem = {
      project: {
        id: `demo-project-${id}`,
        name: input.projectName,
        rootPath: input.projectRootPath,
        createdAt: now,
        updatedAt: now,
      },
      task: {
        id,
        projectId: `demo-project-${id}`,
        title: input.title,
        originalInstruction: input.originalInstruction,
        status: "active",
        createdAt: now,
        updatedAt: now,
        completedAt: null,
      },
    };
    items = [...items, item];
    return structuredClone(item);
  },
  async confirmCompleted(taskId: string) {
    const item = items.find(({ task }) => task.id === taskId);
    if (!item) throw new Error("Task not found");
    const completed = structuredClone(item);
    completed.task.status = "completed";
    completed.task.updatedAt += 1;
    completed.task.completedAt = completed.task.updatedAt;
    items = items.filter(({ task }) => task.id !== taskId);
    return completed;
  },
};
