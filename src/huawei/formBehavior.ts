import type { ProviderFormExtension } from "@/config/formExtensions";

// Shared forms supply context; provider-specific save rules stay in this module.
export const huaweiFormBehavior: Pick<
  ProviderFormExtension,
  "forceDesktopProxy" | "onSaveMeta"
> = {
  forceDesktopProxy: true,
  onSaveMeta: ({ appId, presetAppTarget, selectedAccountId }) => ({
    appTarget:
      appId === "claude-desktop"
        ? "claude_desktop"
        : (presetAppTarget ?? "claude"),
    selectedAccountId: selectedAccountId ?? undefined,
  }),
};
