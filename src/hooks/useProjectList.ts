import { useMemo } from "react";
import type { SessionMeta } from "@/types";

export interface ProjectInfo {
  projectDir: string;
  projectName: string;
  count: number;
  lastActiveAt: number;
}

function getBaseName(value: string): string {
  const trimmed = value.trim();
  if (!trimmed) return "";
  const normalized = trimmed.replace(/[\\/]+$/, "");
  const parts = normalized.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || trimmed;
}

export function useProjectList(
  sessions: SessionMeta[],
  /** Provider ID, or `"all"` / undefined for no filter */
  providerFilter?: string,
): ProjectInfo[] {
  return useMemo(() => {
    const filtered =
      providerFilter && providerFilter !== "all"
        ? sessions.filter((s) => s.providerId === providerFilter)
        : sessions;

    const map = new Map<string, { count: number; lastActiveAt: number }>();

    for (const session of filtered) {
      const dir = session.projectDir;
      if (!dir) continue;

      const existing = map.get(dir);
      if (existing) {
        existing.count += 1;
        const ts = session.lastActiveAt ?? session.createdAt ?? 0;
        if (ts > existing.lastActiveAt) {
          existing.lastActiveAt = ts;
        }
      } else {
        map.set(dir, {
          count: 1,
          lastActiveAt: session.lastActiveAt ?? session.createdAt ?? 0,
        });
      }
    }

    const entries = Array.from(map.entries());

    // Sort by most recent activity, keep top 50
    entries.sort((a, b) => b[1].lastActiveAt - a[1].lastActiveAt);
    const top50 = entries.slice(0, 50);

    return top50.map(([dir, info]) => ({
      projectDir: dir,
      projectName: getBaseName(dir),
      count: info.count,
      lastActiveAt: info.lastActiveAt,
    }));
  }, [sessions, providerFilter]);
}
