import installers from "@/config/appInstallers.json";
import type { InstallMethod } from "@/lib/api/settings";
import type { ToolName } from "./useToolManagement";

export type InstallApp = ToolName | "workbuddy" | "claude-desktop";

export type InstallPlatform = "macos" | "linux" | "windows";

type InstallChannel = { name: string } & (
  | { command: string; method?: InstallMethod; guide?: never }
  | { guide: string; command?: never; method?: never }
);

type PlatformChannels = {
  all?: InstallChannel[];
  unix?: InstallChannel[];
} & Partial<Record<InstallPlatform, InstallChannel[]>>;

const command = (name: string, command: string): InstallChannel => ({
  name,
  command,
});

const guide = (name: string, url: string): InstallChannel => ({
  name,
  guide: url,
});

const installersByApp: Partial<
  Record<
    InstallApp,
    Partial<Record<InstallMethod, { command: string; platforms: string[] }>>
  >
> = installers;

const powershell = (url: string) => command("PowerShell", `irm ${url} | iex`);
const packageChannel = (
  app: InstallApp,
  method: InstallMethod,
  name: string,
): InstallChannel => ({
  name,
  method,
  command: installersByApp[app]![method]!.command,
});

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
  codebuddy: {
    china: "https://www.codebuddy.cn/",
    global: "https://www.codebuddy.ai/",
  },
  workbuddy: {
    china: "https://www.workbuddy.cn/",
    global: "https://www.workbuddy.ai/",
  },
  "claude-desktop": "https://claude.com/download",
  copilot:
    "https://docs.github.com/en/copilot/how-tos/copilot-cli/set-up-copilot-cli/install-copilot-cli",
};

const channels: Record<InstallApp, PlatformChannels> = {
  codebuddy: {
    all: [
      packageChannel("codebuddy", "npm", "npm"),
      packageChannel("codebuddy", "pnpm", "pnpm"),
      packageChannel("codebuddy", "yarn", "Yarn"),
      packageChannel("codebuddy", "bun", "Bun"),
    ],
    windows: [powershell("https://www.codebuddy.cn/cli/install.ps1")],
  },
  workbuddy: {},
  "claude-desktop": {},
  claude: {
    all: [packageChannel("claude", "npm", "npm")],
    linux: [
      guide(
        "APT / DNF / APK",
        "https://code.claude.com/docs/en/setup#install-with-linux-package-managers",
      ),
    ],
    windows: [
      powershell("https://claude.ai/install.ps1"),
      command(
        "Command Prompt (CMD)",
        "curl -fsSL https://claude.ai/install.cmd -o install.cmd && install.cmd && del install.cmd",
      ),
      command("WinGet", "winget install --id Anthropic.ClaudeCode --exact"),
    ],
  },
  codex: {
    all: [packageChannel("codex", "npm", "npm")],
    windows: [powershell("https://chatgpt.com/codex/install.ps1")],
  },
  gemini: {
    all: [
      packageChannel("gemini", "npm", "npm"),
      guide(
        "Anaconda",
        "https://geminicli.com/docs/get-started/installation/#install-gemini-cli",
      ),
    ],
    macos: [command("MacPorts", "sudo port install gemini-cli")],
  },
  grok: {
    all: [packageChannel("grok", "npm", "npm")],
    windows: [powershell("https://x.ai/cli/install.ps1")],
  },
  // OpenCode 2 uses a different npm package and Homebrew formula from OpenCode 1.
  opencode: {
    all: [
      guide("CLI binaries", "https://opencode.ai/docs/#cli"),
      guide("Desktop downloads", "https://opencode.ai/docs/#desktop"),
      guide("Docker", "https://opencode.ai/docs/#docker"),
    ],
    windows: [packageChannel("opencode", "npm", "npm (OpenCode 1)")],
    unix: [
      packageChannel("opencode", "npm-legacy", "npm ≤ 11.15"),
      packageChannel("opencode", "npm-modern", "npm 11.16+"),
      packageChannel("opencode", "bun", "Bun"),
      packageChannel("opencode", "pnpm-build", "pnpm"),
      packageChannel("opencode", "yarn", "Yarn Classic"),
      command("Vite+", "vp install -g @opencode/cli"),
      command("mise ≥ 2026.10.9", "mise use -g opencode"),
    ],
    linux: [
      command("AUR (Arch Linux)", "paru -S opencode-beta"),
      command("AUR (yay)", "yay -S opencode-beta"),
    ],
  },
  openclaw: {
    all: [
      packageChannel("openclaw", "npm-legacy", "npm ≤ 11.15"),
      packageChannel("openclaw", "npm-modern", "npm 11.16+"),
      guide("Docker", "https://docs.openclaw.ai/install/docker"),
      guide("From source", "https://docs.openclaw.ai/install#from-source"),
    ],
    unix: [
      packageChannel("openclaw", "pnpm-build", "pnpm"),
      packageChannel("openclaw", "bun", "Bun"),
      guide("Nix", "https://docs.openclaw.ai/install/nix"),
    ],
    linux: [
      guide("Podman", "https://docs.openclaw.ai/install/podman"),
      guide("Ansible", "https://docs.openclaw.ai/install/ansible"),
    ],
    windows: [powershell("https://openclaw.ai/install.ps1")],
  },
  hermes: {
    all: [
      guide(
        "Docker",
        "https://hermes-agent.nousresearch.com/docs/getting-started/platform-support",
      ),
    ],
    unix: [
      guide(
        "Nix",
        "https://hermes-agent.nousresearch.com/docs/getting-started/nix-setup",
      ),
    ],
    windows: [
      guide(
        "Desktop (MSIX)",
        "https://hermes-agent.nousresearch.com/docs/getting-started/installation#desktop-packages-on-macos-or-windows",
      ),
      command(
        "PowerShell",
        "iex (irm https://hermes-agent.nousresearch.com/install.ps1)",
      ),
    ],
  },
  pi: {
    all: [packageChannel("pi", "npm", "npm")],
    windows: [powershell("https://pi.dev/install.ps1")],
  },
  mcode: {
    all: [packageChannel("mcode", "npm", "npm")],
    windows: [powershell("https://filecdn.minimax.chat/public/install.ps1")],
  },
  kimi: {
    all: [packageChannel("kimi", "npm", "npm")],
    windows: [powershell("https://code.kimi.com/kimi-code/install.ps1")],
  },
  dsh: {
    all: [
      packageChannel("dsh", "npm", "npm"),
      command("npx (Web UI)", "npx @deepseek-ai/dsh web"),
      guide(
        "From source",
        "https://github.com/deepseek-ai/deepseek-harness#run-from-source",
      ),
    ],
  },
  agy: {
    windows: [powershell("https://antigravity.google/cli/install.ps1")],
  },
  omp: {
    windows: [powershell("https://omp.sh/install.ps1")],
    all: [packageChannel("omp", "bun", "Bun")],
    unix: [
      command("Homebrew", "brew install can1357/tap/omp"),
      command("Nix", "nix profile add github:can1357/oh-my-pi"),
      command("mise", "mise use -g github:can1357/oh-my-pi"),
    ],
  },
  copilot: {
    all: [
      packageChannel("copilot", "npm", "npm"),
      guide(
        "GitHub Releases",
        "https://github.com/github/copilot-cli/releases/latest",
      ),
    ],
    linux: [command("Homebrew", "brew install --cask copilot-cli")],
    windows: [command("WinGet", "winget install --id GitHub.Copilot --exact")],
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
  ].filter(
    (channel) =>
      !channel.method || direct?.[channel.method]?.platforms.includes(platform),
  );
}
