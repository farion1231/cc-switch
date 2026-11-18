import { invoke } from "@tauri-apps/api/core";
export interface ProfileKey {
  root: "default" | "managed";
  account: string;
  org: string;
}
export const profileId = (key: ProfileKey) =>
  JSON.stringify([key.root, key.account, key.org]);
export interface Preview {
  token: string;
  canApply: boolean;
  items: {
    id: string;
    disposition:
      | { kind: "ready" | "exists" }
      | { kind: "blocked"; reason: Problem };
  }[];
}
export type Problem =
  | "missingPrompt"
  | "missingDirectory"
  | "unsupportedTask"
  | "expired";
export interface Task {
  id: string;
  title: string;
  cwd: string;
  enabled: boolean;
  schedule:
    | { kind: "cron"; expression: string }
    | { kind: "once"; at: number }
    | { kind: "manual" };
  promptAvailable: boolean;
  problem: Problem | null;
}
export interface Profile {
  key: ProfileKey;
  lastKnownAccount: boolean;
  tasks: Task[];
  warnings: string[];
}
export interface Inventory {
  profiles: Profile[];
  orphanPrompts: string[];
  warnings: string[];
  writesSupported: boolean;
}
export interface Request {
  source: ProfileKey;
  target: ProfileKey;
  taskIds: string[];
}
export interface Receipt {
  restoredTasks: string[];
  skipped: string[];
  backupPath: string | null;
  error: string | null;
}
export const desktopTasksApi = {
  inventory: () => invoke<Inventory>("list_desktop_task_profiles"),
  preview: (request: Request) =>
    invoke<Preview>("preview_desktop_task_recovery", { request }),
  apply: (request: Request, token: string) =>
    invoke<Receipt>("apply_desktop_task_recovery", { request, token }),
  prompt: (id: string) => invoke<string>("read_desktop_saved_prompt", { id }),
};
