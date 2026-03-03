#!/usr/bin/env bash
set -euo pipefail

BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LA_DIR="${HOME}/Library/LaunchAgents"
mkdir -p "$LA_DIR"

cat > "$LA_DIR/com.wousp.codex-auth-normalizer.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>com.wousp.codex-auth-normalizer</string>
<key>ProgramArguments</key><array><string>${BASE_DIR}/codex-auth-normalize.sh</string></array>
<key>RunAtLoad</key><true/>
<key>StartInterval</key><integer>5</integer>
<key>WatchPaths</key><array><string>${HOME}/.codex/auth.json</string></array>
<key>StandardOutPath</key><string>/tmp/codex-auth-normalizer.out.log</string>
<key>StandardErrorPath</key><string>/tmp/codex-auth-normalizer.err.log</string>
</dict></plist>
PLIST

cat > "$LA_DIR/com.wousp.ccswitch-localhost-guard.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>com.wousp.ccswitch-localhost-guard</string>
<key>ProgramArguments</key><array><string>${BASE_DIR}/ccswitch-localhost-guard.sh</string></array>
<key>RunAtLoad</key><true/>
<key>StartInterval</key><integer>45</integer>
<key>StandardOutPath</key><string>/tmp/ccswitch-localhost-guard.out.log</string>
<key>StandardErrorPath</key><string>/tmp/ccswitch-localhost-guard.err.log</string>
</dict></plist>
PLIST

cat > "$LA_DIR/com.wousp.ccswitch-breaker-guard.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>com.wousp.ccswitch-breaker-guard</string>
<key>ProgramArguments</key><array><string>${BASE_DIR}/ccswitch-breaker-guard.sh</string></array>
<key>RunAtLoad</key><true/>
<key>StartInterval</key><integer>60</integer>
<key>StandardOutPath</key><string>/tmp/ccswitch-breaker-guard.out.log</string>
<key>StandardErrorPath</key><string>/tmp/ccswitch-breaker-guard.err.log</string>
</dict></plist>
PLIST

for label in com.wousp.codex-auth-normalizer com.wousp.ccswitch-localhost-guard com.wousp.ccswitch-breaker-guard; do
  launchctl unload "$LA_DIR/${label}.plist" >/dev/null 2>&1 || true
  launchctl load -w "$LA_DIR/${label}.plist"
done

echo "installed launch agents under $LA_DIR"
