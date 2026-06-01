import type { FeatureVisibility } from "@/types";

export const DEFAULT_FEATURE_VISIBILITY: FeatureVisibility = {
  mcp: true,
  prompts: true,
  sessions: true,
  skills: true,
};

export function normalizeFeatureVisibility(
  value?: Partial<FeatureVisibility> | null,
): FeatureVisibility {
  return {
    ...DEFAULT_FEATURE_VISIBILITY,
    ...(value ?? {}),
  };
}
