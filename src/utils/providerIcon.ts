import type { AppId } from "@/lib/api/types";

/**
 * Normalize provider icons before rendering.
 *
 * Some app-specific creation flows historically stored a protocol/app icon
 * automatically. An empty icon color distinguishes those legacy defaults from
 * an icon explicitly selected by the user. Returning undefined delegates to
 * ProviderIcon's provider-name initials fallback.
 */
export function resolveProviderIcon(
  appId: AppId,
  icon?: string,
  iconColor?: string,
): string | undefined {
  const normalizedIcon = icon?.trim();
  if (!normalizedIcon) return undefined;

  if (
    appId === "cursor" &&
    (normalizedIcon === "openai" || normalizedIcon === "anthropic") &&
    !iconColor?.trim()
  ) {
    return undefined;
  }

  if (
    appId === "grokbuild" &&
    normalizedIcon === "grok" &&
    !iconColor?.trim()
  ) {
    return undefined;
  }

  return normalizedIcon;
}
