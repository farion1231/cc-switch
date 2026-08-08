/**
 * Normalize user-entered search text without applying locale-specific rules.
 * This keeps matching deterministic across macOS, Windows, and Linux clients.
 */
export function normalizeSearchQuery(value: string): string {
  return value.trim().toLowerCase();
}

/**
 * Return true when the query is empty or occurs in at least one candidate.
 * Candidates may be absent because different records expose different fields.
 */
export function matchesSearchQuery(
  query: string,
  ...candidates: Array<string | null | undefined>
): boolean {
  const normalizedQuery = normalizeSearchQuery(query);
  if (!normalizedQuery) return true;

  return candidates.some(
    (candidate) =>
      typeof candidate === "string" &&
      candidate.toLowerCase().includes(normalizedQuery),
  );
}
