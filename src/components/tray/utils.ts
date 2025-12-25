export const clamp = (value: number, min = 0, max = 100) =>
  Math.max(min, Math.min(max, value));

export const formatHost = (url?: string) => {
  if (!url) return "";
  try {
    const host = new URL(url);
    return host.hostname.replace(/^www\./, "");
  } catch {
    return url.replace(/^https?:\/\//, "");
  }
};

export const formatPercentage = (used?: number, total?: number) => {
  if (!used || !total) return undefined;
  if (total === 0) return 0;
  return Math.min(100, Math.max(0, Math.round((used / total) * 100)));
};
