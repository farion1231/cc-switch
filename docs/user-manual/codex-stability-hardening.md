# Codex Stability Hardening (CLI/App/IDE via CC Switch)

This playbook captures a practical hardening set for local proxy setups where Codex routes through CC Switch.

## What it solves

- Auth schema drift in `~/.codex/auth.json` (`api_key` vs `apikey`) causing immediate CLI failures.
- Dummy-key poisoning (`sk-cc-switch-proxy`) being synced to upstream provider auth, triggering `401 invalid_api_key` and circuit open.
- WebKit cache contamination where CC Switch shows localhost:3000 content (for example Obsidian preview fallback text).

## Scripts

- `scripts/codex-stability/codex-auth-normalize.sh`
- `scripts/codex-stability/ccswitch-breaker-guard.sh`
- `scripts/codex-stability/ccswitch-localhost-guard.sh`
- `scripts/codex-stability/install-launchagents.sh`

## Install

```bash
cd scripts/codex-stability
chmod +x *.sh
./install-launchagents.sh
```

## Quick checks

```bash
scripts/codex-stability/codex-auth-normalize.sh
scripts/codex-stability/ccswitch-breaker-guard.sh --dry-run
scripts/codex-stability/ccswitch-localhost-guard.sh --dry-run
```

## Notes

- Guards intentionally avoid modifying account rows directly except targeted provider health/cooldown reset for Codex.
- For white-screen class issues, cache quarantine is used instead of deleting `~/.cc-switch/cc-switch.db`.
