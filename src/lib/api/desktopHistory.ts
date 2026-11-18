import { invoke } from "@tauri-apps/api/core";
import type { SessionMeta } from "@/types";
export interface DesktopProfileKey {
  root: "default" | "managed";
  account: string;
  org: string;
}
export type TranscriptLink =
  | { kind: "available"; id: string; path: string }
  | { kind: "missing" | "ambiguous"; id: string };
export interface DesktopSession {
  id: string;
  title: string;
  cwd: string;
  archived: boolean;
  updatedAt: number | null;
  transcripts: TranscriptLink[];
}
export interface DesktopProfile {
  key: DesktopProfileKey;
  lastKnownAccount: boolean;
  sessions: DesktopSession[];
  warnings: string[];
}
export interface DesktopInventory {
  profiles: DesktopProfile[];
  warnings: string[];
}
export const profileId = (key: DesktopProfileKey) =>
  JSON.stringify([key.root, key.account, key.org]);
export const desktopHistoryApi = {
  inventory: () => invoke<DesktopInventory>("list_desktop_history"),
  openTranscript: (
    profile: DesktopProfileKey,
    sessionId: string,
    transcriptId: string,
  ) =>
    invoke<SessionMeta>("open_desktop_history_transcript", {
      profile,
      sessionId,
      transcriptId,
    }),
};
