export const REQUEST_HEADER_DRAFT_PREFIX = "draft-header:";

export function normalizeRequestHeaders(
  headers: Record<string, string>,
): Record<string, string> {
  const normalized: Record<string, string> = {};
  for (const [key, value] of Object.entries(headers)) {
    const trimmedKey = key.trim();
    if (trimmedKey && !key.startsWith(REQUEST_HEADER_DRAFT_PREFIX)) {
      normalized[trimmedKey] = value;
    }
  }
  return normalized;
}

export function findRequestHeaderValue(
  headers: Record<string, string>,
  name: string,
): string | undefined {
  const normalizedName = name.toLowerCase();
  return Object.entries(headers).find(
    ([key]) => key.toLowerCase() === normalizedName,
  )?.[1];
}
