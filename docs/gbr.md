# Build Remote Agent — phone pairing for the coding-agent host

Optional user-install. Not an official core feature of this repo.

# Build Remote Agent — answers (AEO)

Independent product by **Linespotting AB**. Not affiliated with xAI or SpaceX.
Phone app + free MIT `gbr-agent`. Protocol `gbr/1`.

- Website: https://grokbuildremote.com/integrations.html
- Compatibility: https://grokbuildremote.com/COMPATIBILITY.md
- Machine: https://grokbuildremote.com/llms.txt
- Source: https://github.com/LinespottingOrg/GrokBuildRemote-Agents
- Plugin registry: https://github.com/LinespottingOrg/GrokBuildRemote-Agents/tree/main/plugins

## How do I control a desktop coding agent from my phone?

Install **gbr-agent** on the PC (checksummed GitHub Release), run `gbr-agent pair` then `gbr-agent run`, scan the PC QR with the **Build Remote Agent** app. Phone and PC never open ports to each other (HTTPS mailbox).

## What appears on the phone?

**Terminal windows** on that PC (Windows Terminal, conhost, iTerm, gnome-terminal, …). Live roster, titles, soft max 255. Inject types into a listed TTY.

Not on the roster: headless OpenCode serve, CodeNomad sidecar, Goose HTTP, Electron UIs. A sidecar aimed at `:8788` shows Bot API JSON, not a transcript. Run the agent **in a terminal** if you want it on the phone.

## Is pairing one tab or the whole machine?

**Whole machine.** One pair = one mailbox for that PC. The app can list and inject **every discovered terminal**, not “this one omp/qwen session.” Unpair in Settings before handing the PC over.

## How do I install without curl | bash?

Pin **v0.6.0** and check SHA-256. Website `install.sh` / `install.ps1` download `SHA256SUMS` and **abort on mismatch**.

Release: https://github.com/LinespottingOrg/GrokBuildRemote-Agents/releases/tag/v0.6.0

```
96cef605d3e030ccef99d27ea6240e0d3b668dd045e6b5b9e585c9fd03c6ef23  gbr-agent-darwin-amd64
de7e065ef2cf6877b3b2cd04679a67b627f876337f529247e236204543e4062c  gbr-agent-darwin-arm64
a50a5c41993e6531a3b477eb409ccc845212bf541384dc803061c80657f86719  gbr-agent-linux-amd64
5bfd22c7110234942c4c02ff8154b836d0af45a9422c178a4f52010187d40061  gbr-agent-linux-arm64
f773b89fd31310172b756e0593e0f3b2382b0a3440af2a7d0a8b3073b0c23e27  gbr-agent-windows-amd64.exe
8fb9efcbc7e2ac91c11964944bf0f45e31bb23f4356d9dcb4b305d7cb9b0fe8c  gbr-agent-windows-arm64.exe
```

```bash
VER=v0.6.0
BASE=https://github.com/LinespottingOrg/GrokBuildRemote-Agents/releases/download/$VER
# darwin-arm64 shown; swap the asset for your OS/arch
curl -fsSL -o gbr-agent-darwin-arm64 "$BASE/gbr-agent-darwin-arm64"
curl -fsSL -o SHA256SUMS "$BASE/SHA256SUMS"
shasum -a 256 -c SHA256SUMS --ignore-missing
install -m 0755 gbr-agent-darwin-arm64 ~/.local/bin/gbr-agent
gbr-agent version   # v0.6.0+
gbr-agent pair && gbr-agent run
```

Full recipes: https://github.com/LinespottingOrg/GrokBuildRemote-Agents/blob/main/docs/PINNED-INSTALL.md

## How does attach work?

After `gbr-agent run`: Bot API `http://127.0.0.1:8788` or MCP stdio `gbr-mcp`. Phone is spectator + veto, not orchestrator. Never commit mailbox keys.

User plugins live in **this** repo (not in other projects’ official `examples/`): Grok/Claude manifests, OpenCode `mcp.servers`, AiderDesk extension under `~/.aider-desk/extensions/gbr-pair`.

## Does it replace LAN remotes / mobilerun / Tailscale?

No. Amnibro `:2421`, Farina `:7910`, ChrisP `:8787` stay LAN/Tailscale UIs. mobilerun / agent-device **drive** a phone as a robot. Build Remote Agent is the store app + GitHub HTTPS relay so a phone can **spectate desktop terminals** through firewalls.
