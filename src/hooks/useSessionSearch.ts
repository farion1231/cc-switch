import { useCallback, useMemo } from "react";
import type { SessionMeta } from "@/types";

interface UseSessionSearchOptions {
  sessions: SessionMeta[];
  providerFilter: string;
  providerIds?: readonly string[];
}

interface UseSessionSearchResult {
  search: (query: string) => SessionMeta[];
}

/** Literal, case-insensitive metadata search. The page owns result ordering. */
export function useSessionSearch({
  sessions,
  providerFilter,
  providerIds,
}: UseSessionSearchOptions): UseSessionSearchResult {
  // Prepare at most once per session list, only after the first nonempty search.
  // Application filters never rebuild metadata or create substring postings.
  const getRecords = useMemo(() => {
    let records: { session: SessionMeta; text: string }[] | undefined;
    return () =>
      (records ??= sessions.map((session) => ({
        session,
        text: [
          session.sessionId,
          session.title,
          session.summary,
          session.projectDir,
          session.sourcePath,
        ]
          .filter(Boolean)
          .join("\n")
          .toLowerCase(),
      })));
  }, [sessions]);

  const search = useCallback(
    (query: string): SessionMeta[] => {
      const allowed = (session: SessionMeta) =>
        (providerFilter === "all" || session.providerId === providerFilter) &&
        (!providerIds || providerIds.includes(session.providerId));
      const terms = query.toLowerCase().trim().split(/\s+/).filter(Boolean);
      if (terms.length === 0) return sessions.filter(allowed);
      return getRecords()
        .filter(
          ({ session, text }) =>
            allowed(session) && terms.every((term) => text.includes(term)),
        )
        .map(({ session }) => session);
    },
    [sessions, getRecords, providerFilter, providerIds],
  );

  return { search };
}
