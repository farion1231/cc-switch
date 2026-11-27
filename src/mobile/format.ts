export function formatCompactNumber(n: number): string {
  if (!Number.isFinite(n)) return "0";
  if (n >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(2)}B`;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(Math.round(n));
}

export function formatUsd(raw: string | number): string {
  const n = typeof raw === "number" ? raw : Number.parseFloat(raw);
  if (!Number.isFinite(n) || n === 0) return "$0.00";
  if (n < 0.01) return `$${n.toFixed(4)}`;
  if (n < 100) return `$${n.toFixed(2)}`;
  return `$${n.toFixed(1)}`;
}

export function formatPercent(fraction: number): string {
  if (!Number.isFinite(fraction) || fraction <= 0) return "0%";
  return `${(fraction * 100).toFixed(1)}%`;
}

export function formatLatency(ms: number | null | undefined): string {
  if (ms == null || !Number.isFinite(ms)) return "—";
  if (ms >= 1000) return `${(ms / 1000).toFixed(2)}s`;
  return `${Math.round(ms)}ms`;
}

export function formatTps(
  tps: number | null | undefined,
  estimated = false,
): string {
  if (tps == null || !Number.isFinite(tps) || tps <= 0) return "—";
  const prefix = estimated ? "≈" : "";
  return `${prefix}${tps.toFixed(1)} tok/s`;
}

export function formatUptime(seconds: number): string {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}

export function formatRelativeTime(unixSec: number): string {
  const diff = Math.max(0, Math.floor(Date.now() / 1000) - unixSec);
  if (diff < 60) return `${diff}s`;
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
  return `${Math.floor(diff / 86400)}d`;
}

export function computeRowTps(row: {
  speedOutputTokens: number;
  speedGenerationMs: number;
  estSpeedOutputTokens: number;
  estSpeedDurationMs: number;
}): { tps: number | null; estimated: boolean } {
  if (row.speedOutputTokens > 0 && row.speedGenerationMs > 0) {
    return {
      tps: row.speedOutputTokens / (row.speedGenerationMs / 1000),
      estimated: false,
    };
  }
  if (row.estSpeedOutputTokens > 0 && row.estSpeedDurationMs > 0) {
    return {
      tps: row.estSpeedOutputTokens / (row.estSpeedDurationMs / 1000),
      estimated: true,
    };
  }
  return { tps: null, estimated: false };
}

const APP_LABELS: Record<string, string> = {
  claude: "Claude Code",
  "claude-desktop": "Claude Desktop",
  codex: "Codex",
  gemini: "Gemini CLI",
  grokbuild: "Grok Build",
  opencode: "OpenCode",
  openclaw: "OpenClaw",
  hermes: "Hermes",
  pi: "Pi",
  omp: "OMP",
  mcode: "MCode",
  kimi: "Kimi Code",
  dsh: "DeepSeek",
  agy: "Antigravity",
  copilot: "Copilot",
  workbuddy: "WorkBuddy",
  codebuddy: "CodeBuddy",
};

export function formatAppName(app: string): string {
  return APP_LABELS[app] ?? app;
}
