export function isHttpsIconUrl(value?: string): boolean {
  if (!value) return false;

  try {
    return new URL(value).protocol === "https:";
  } catch {
    return false;
  }
}
