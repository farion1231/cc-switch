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
export type Problem = "deleted" | "unverified" | "targetArchived";
export interface Session {
  id: string;
  title: string;
  cwd: string;
  archived: boolean;
}
export interface Profile {
  key: ProfileKey;
  lastKnownAccount: boolean;
  sessions: Session[];
  warnings: string[];
}
export interface Inventory {
  profiles: Profile[];
  warnings: string[];
  writesSupported: boolean;
}
export interface Request {
  source: ProfileKey;
  target: ProfileKey;
  sessionIds: string[];
}
export interface Receipt {
  copiedSessions: string[];
  skipped: string[];
  error: string | null;
}
export const desktopMigrationApi = {
  inventory: () => invoke<Inventory>("list_desktop_migration_profiles"),
  preview: (request: Request) =>
    invoke<Preview>("preview_desktop_session_migration", { request }),
  apply: (request: Request, token: string) =>
    invoke<Receipt>("apply_desktop_session_migration", { request, token }),
};
