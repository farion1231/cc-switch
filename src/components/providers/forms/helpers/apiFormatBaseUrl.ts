import type { ClaudeApiFormat } from "@/types";

type KnownApiProtocol = "anthropic" | "openai";

interface KnownEndpointPair {
  anthropic: string;
  openai: string;
}

// Only endpoint pairs whose path is part of the provider's documented API
// contract belong here. Keep the list explicit so unknown/custom relays are
// never rewritten based on a loose host or suffix heuristic.
const KNOWN_ENDPOINT_PAIRS: readonly KnownEndpointPair[] = [
  {
    anthropic: "https://api.minimax.cn/anthropic",
    openai: "https://api.minimax.cn/v1",
  },
  {
    anthropic: "https://api.minimax.io/anthropic",
    openai: "https://api.minimax.io/v1",
  },
  {
    anthropic: "https://api.minimaxi.com/anthropic",
    openai: "https://api.minimaxi.com/v1",
  },
];

function normalizeBaseUrl(baseUrl: string): string | null {
  const trimmed = baseUrl.trim();
  if (!trimmed) return null;

  try {
    const url = new URL(trimmed);
    if (url.search || url.hash || url.username || url.password) return null;

    const pathname = url.pathname.replace(/\/+$/, "");
    return `${url.protocol}//${url.host}${pathname}`;
  } catch {
    return null;
  }
}

function protocolForApiFormat(
  apiFormat: ClaudeApiFormat | null | undefined,
): KnownApiProtocol | null {
  if (apiFormat === "anthropic") return "anthropic";
  if (apiFormat === "openai_chat" || apiFormat === "openai_responses") {
    return "openai";
  }
  return null;
}

/**
 * Returns the matching URL from an explicitly known endpoint pair, or null
 * when the current URL is custom, ambiguous, or already correct.
 */
export function resolveKnownBaseUrlForApiFormat(
  baseUrl: string,
  apiFormat: ClaudeApiFormat | null | undefined,
): string | null {
  const targetProtocol = protocolForApiFormat(apiFormat);
  const normalizedCurrent = normalizeBaseUrl(baseUrl);
  if (!targetProtocol || !normalizedCurrent) return null;

  for (const pair of KNOWN_ENDPOINT_PAIRS) {
    const normalizedAnthropic = normalizeBaseUrl(pair.anthropic);
    const normalizedOpenAI = normalizeBaseUrl(pair.openai);
    if (
      normalizedCurrent !== normalizedAnthropic &&
      normalizedCurrent !== normalizedOpenAI
    ) {
      continue;
    }

    const target = pair[targetProtocol];
    return normalizeBaseUrl(target) === normalizedCurrent ? null : target;
  }

  return null;
}

export function resolveKnownOpencodeBaseUrl(
  baseUrl: string,
  npm: string,
): string | null {
  const apiFormat: ClaudeApiFormat | null =
    npm === "@ai-sdk/anthropic"
      ? "anthropic"
      : npm === "@ai-sdk/openai-compatible"
        ? "openai_chat"
        : npm === "@ai-sdk/openai"
          ? "openai_responses"
          : null;

  return resolveKnownBaseUrlForApiFormat(baseUrl, apiFormat);
}
