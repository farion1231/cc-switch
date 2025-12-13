const randomSegment = () =>
  Math.random().toString(16).slice(2, 8).padEnd(6, "0");

const fallbackId = (prefix: string) =>
  `${prefix}-${Date.now().toString(36)}-${randomSegment()}`;

export const generateRuntimeId = (prefix = "id"): string => {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    try {
      return crypto.randomUUID();
    } catch {
      // ignore and use fallback
    }
  }
  return fallbackId(prefix);
};

export const generateConfigDirectorySetId = (): string =>
  generateRuntimeId("configset");
