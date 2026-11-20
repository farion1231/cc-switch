import installers from "@/config/appInstallers.json";
import type { InstallMethod } from "@/lib/api/settings";
import type { ToolName } from "./useToolManagement";

export type InstallApp = ToolName | "workbuddy" | "claude-desktop";

export type InstallPlatform = "macos" | "linux" | "windows";

type InstallChannel = { name: string; command: string; method?: InstallMethod };

type PlatformChannels = {
  all?: InstallChannel[];
  unix?: InstallChannel[];
} & Partial<Record<InstallPlatform, InstallChannel[]>>;

const command = (name: string, command: string): InstallChannel => ({
  name,
  command,
});

const installersByApp: Partial<
  Record<
    InstallApp,
    Partial<Record<InstallMethod, { command: string; platforms: string[] }>>
  >
> = installers;

const powershell = (url: string) => command("PowerShell", `irm ${url} | iex`);
const npm = (args: string) => command("npm", `npm install -g ${args}`);

export const OFFICIAL_INSTALL_PAGES: Record<
  InstallApp,
  string | { china: string; global: string }
> = {
  claude: "https://claude.com/product/claude-code",
  codex: "https://github.com/openai/codex/releases/latest",
  gemini: "https://geminicli.com/docs/get-started/installation/",
  grok: "https://x.ai/build",
  opencode: "https://opencode.ai/docs/#cli",
  openclaw: "https://docs.openclaw.ai/install",
  hermes: "https://hermes-agent.nousresearch.com/desktop",
  pi: "https://pi.dev/",
  mcode: {
    china: "https://code.minimax.cn/about",
    global: "https://code.minimax.io/about",
  },
  kimi: "https://www.kimi.com/products/download",
  dsh: "https://www.deepseek.com/en/harness/",
  agy: "https://antigravity.google/download#antigravity-cli",
  omp: "https://github.com/can1357/oh-my-pi/releases/latest",
  workbuddy: {
    china: "https://www.workbuddy.cn/",
    global: "https://www.workbuddy.ai/",
  },
  "claude-desktop": "https://claude.com/download",
  copilot:
    "https://docs.github.com/en/copilot/how-tos/copilot-cli/set-up-copilot-cli/install-copilot-cli",
};

const channels: Record<InstallApp, PlatformChannels> = {
  workbuddy: {},
  "claude-desktop": {},
  claude: {
    all: [
      npm(
        "@anthropic-ai/claude-code@latest --ignore-scripts=false --include=optional --allow-scripts=@anthropic-ai/claude-code",
      ),
    ],
    windows: [
      powershell("https://claude.ai/install.ps1"),
      command("WinGet", "winget install Anthropic.ClaudeCode"),
    ],
  },
  codex: {
    all: [npm("@openai/codex@latest")],
    windows: [powershell("https://chatgpt.com/codex/install.ps1")],
  },
  gemini: {
    all: [npm("@google/gemini-cli@latest")],
    macos: [command("MacPorts", "sudo port install gemini-cli")],
  },
  grok: {
    all: [
      npm(
        "@xai-official/grok@latest --ignore-scripts=false --include=optional --allow-scripts=@xai-official/grok",
      ),
    ],
    windows: [powershell("https://x.ai/cli/install.ps1")],
  },
  // OpenCode 2 uses a different npm package and Homebrew formula from OpenCode 1.
  opencode: {
    windows: [
      command(
        "npm (OpenCode 1)",
        "npm install -g opencode-ai@latest --ignore-scripts=false --include=optional --allow-scripts=opencode-ai",
      ),
    ],
    unix: [
      command(
        "npm ≤ 11.15",
        "npm install -g --ignore-scripts=false @opencode/cli",
      ),
      command(
        "npm 11.16+",
        "npm install -g --ignore-scripts=false --allow-scripts=@opencode/cli @opencode/cli",
      ),
      command("Bun", "bun install -g --trust @opencode/cli"),
      command("pnpm", "pnpm add -g --allow-build=@opencode/cli @opencode/cli"),
      command("Yarn Classic", "yarn global add @opencode/cli"),
      command("mise ≥ 2026.10.9", "mise use -g opencode"),
    ],
    linux: [command("AUR (Arch Linux)", "paru -S opencode-beta")],
  },
  openclaw: {
    all: [
      command("npm ≤ 11.15", "npm install -g openclaw@latest"),
      command(
        "npm 11.16+",
        "npm install -g openclaw@latest --allow-scripts=openclaw",
      ),
    ],
    windows: [powershell("https://openclaw.ai/install.ps1")],
  },
  hermes: {
    windows: [
      command(
        "PowerShell",
        "iex (irm https://hermes-agent.nousresearch.com/install.ps1)",
      ),
    ],
  },
  pi: {
    all: [npm("--ignore-scripts @earendil-works/pi-coding-agent")],
    windows: [powershell("https://pi.dev/install.ps1")],
  },
  mcode: {
    all: [
      npm(
        '@minimax-ai/code@latest --ignore-scripts=false --include=optional "--allow-scripts=@minimax-ai/code,better-sqlite3" --registry=https://registry.npmjs.org/',
      ),
    ],
    windows: [powershell("https://filecdn.minimax.chat/public/install.ps1")],
  },
  kimi: {
    windows: [powershell("https://code.kimi.com/kimi-code/install.ps1")],
  },
  dsh: { all: [npm("@deepseek-ai/dsh@latest --engine-strict")] },
  agy: {
    windows: [powershell("https://antigravity.google/cli/install.ps1")],
  },
  omp: {
    windows: [powershell("https://omp.sh/install.ps1")],
    all: [command("Bun", "bun install -g @oh-my-pi/pi-coding-agent@latest")],
  },
  copilot: {
    all: [npm("@github/copilot@latest")],
    windows: [command("WinGet", "winget install GitHub.Copilot")],
  },
};

export function getInstallChannels(
  tool: InstallApp,
  platform: InstallPlatform,
) {
  const entry = channels[tool];
  const direct = installersByApp[tool];
  const nativeChannels = (["curl", "homebrew"] as const).flatMap(
    (method): InstallChannel[] => {
      const selected = direct?.[method];
      if (!selected?.platforms.includes(platform)) return [];
      return [
        {
          name: method === "curl" ? "curl" : "Homebrew",
          command: selected.command,
          method,
        },
      ];
    },
  );
  return [
    ...nativeChannels,
    ...(platform === "windows" ? [] : (entry.unix ?? [])),
    ...(entry[platform] ?? []),
    ...(entry.all ?? []),
  ];
}
