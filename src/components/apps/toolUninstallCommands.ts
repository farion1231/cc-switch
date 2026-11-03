import type { ToolName } from "./useToolManagement";

// Audited against official installation docs, 2026-10-12.
// An npm package does not imply support for every other package manager.
export const UNINSTALL_METHODS = {
  npm: "npm",
  pnpm: "pnpm",
  bun: "bun",
  "brew-formula": "Homebrew (formula)",
  "brew-cask": "Homebrew (cask)",
  hermes: "Hermes",
};

export type UninstallMethod = keyof typeof UNINSTALL_METHODS;
export type UninstallEnvironment = "windows" | "macos" | "linux";

type UninstallCommand = {
  tool: ToolName;
  command: string;
  environments?: UninstallEnvironment[];
  variant?: string;
};

const COMMANDS: Record<UninstallMethod, UninstallCommand[]> = {
  npm: [
    { tool: "dsh", command: "npm uninstall -g @deepseek-ai/dsh" },
    { tool: "claude", command: "npm uninstall -g @anthropic-ai/claude-code" },
    { tool: "codex", command: "npm uninstall -g @openai/codex" },
    { tool: "gemini", command: "npm uninstall -g @google/gemini-cli" },
    { tool: "grok", command: "npm uninstall -g @xai-official/grok" },
    { tool: "opencode", command: "npm uninstall -g opencode-ai" },
    { tool: "openclaw", command: "npm uninstall -g openclaw" },
    { tool: "pi", command: "npm uninstall -g @earendil-works/pi-coding-agent" },
    { tool: "mcode", command: "npm uninstall -g @minimax-ai/code" },
    { tool: "kimi", command: "npm uninstall -g @moonshot-ai/kimi-code" },
  ],
  pnpm: [
    { tool: "opencode", command: "pnpm remove -g opencode-ai" },
    { tool: "openclaw", command: "pnpm remove -g openclaw" },
  ],
  bun: [
    {
      tool: "opencode",
      command: "bun remove -g opencode-ai",
      environments: ["macos", "linux"],
    },
    { tool: "openclaw", command: "bun remove -g openclaw" },
    { tool: "omp", command: "bun remove -g @oh-my-pi/pi-coding-agent" },
  ],
  "brew-formula": [
    {
      tool: "gemini",
      command: "brew uninstall --formula gemini-cli",
      environments: ["macos", "linux"],
    },
    {
      tool: "opencode",
      command: "brew uninstall --formula anomalyco/tap/opencode",
      environments: ["macos", "linux"],
      variant: "anomalyco/tap",
    },
    {
      tool: "opencode",
      command: "brew uninstall --formula opencode",
      environments: ["macos", "linux"],
      variant: "homebrew/core",
    },
  ],
  "brew-cask": [
    {
      tool: "claude",
      command: "brew uninstall --cask claude-code",
      environments: ["macos"],
      variant: "claude-code",
    },
    {
      tool: "claude",
      command: "brew uninstall --cask claude-code@latest",
      environments: ["macos"],
      variant: "claude-code@latest",
    },
    {
      tool: "codex",
      command: "brew uninstall --cask codex",
      environments: ["macos"],
    },
    {
      tool: "agy",
      command: "brew uninstall --cask antigravity-cli",
      environments: ["macos"],
    },
  ],
  hermes: [{ tool: "hermes", command: "hermes uninstall --yes" }],
};

export function uninstallCommandsFor(
  method: UninstallMethod,
  environmentForTool: (tool: ToolName) => UninstallEnvironment | undefined,
) {
  return COMMANDS[method].filter(({ tool, environments }) => {
    const environment = environmentForTool(tool);
    return (
      !environments ||
      (environment !== undefined && environments.includes(environment))
    );
  });
}
