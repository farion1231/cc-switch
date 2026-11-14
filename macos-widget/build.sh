#!/usr/bin/env bash
# 编译、签名 macOS 桌面小组件扩展（CCSwitchWidget.appex），并生成给 Tauri 用的覆盖配置。
#
# 用法：
#   macos-widget/build.sh            只编 appex，输出在 src-tauri/target/macos-widget/
#   macos-widget/build.sh --bundle   编完 appex 后接着 `pnpm tauri build`，把 appex 打进 .app
#                                    （之后的参数原样传给 tauri build，如 --target universal-apple-darwin）
#
# 环境变量：
#   APPLE_SIGNING_IDENTITY  签名证书；不填时取钥匙串里第一张 Developer ID Application，
#                           没有再取 Apple Development。小组件必须用带 Team 的证书签，ad-hoc 不会出现在小组件库里。
#   APPLE_TEAM_ID           Team ID；不填时从证书的 OU 读。App Group = <TeamID>.com.ccswitch.desktop
#   WIDGET_ARCHS            默认 "arm64 x86_64"（和发布用的 universal 包一致）
#
# Tauri 不会单独给 bundle.macOS.files 里的东西签名：appex 在这里按自己的 entitlements（沙盒 +
# App Group）先签好，Tauri 再签外层 app（带上 App Group）并公证，appex 的签名原样保留。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="$ROOT/macos-widget"
OUT="$ROOT/src-tauri/target/macos-widget"
APPEX="$OUT/CCSwitchWidget.appex"
ARCHS="${WIDGET_ARCHS:-arm64 x86_64}"

bundle=0
if [ "${1:-}" = "--bundle" ]; then
  bundle=1
  shift
fi

# ── 签名身份与 Team ID ─────────────────────────────────────────────
IDENTITY="${APPLE_SIGNING_IDENTITY:-}"
if [ -z "$IDENTITY" ]; then
  for kind in "Developer ID Application" "Apple Development"; do
    IDENTITY=$(security find-identity -v -p codesigning | grep "$kind" | grep -oE '"[^"]+"' | head -1 | tr -d '"' || true)
    [ -n "$IDENTITY" ] && break
  done
fi
if [ -z "$IDENTITY" ]; then
  echo "❌ 钥匙串里没有 Developer ID Application / Apple Development 证书，小组件没法签名。" >&2
  exit 1
fi

# 签名一律用证书的 SHA-1：钥匙串里有同名证书（续期后旧的没删）时按名字签会报 ambiguous。
# APPLE_SIGNING_IDENTITY 本身也可以是 SHA-1（Tauri 支持这种写法）。
if [[ "$IDENTITY" =~ ^[0-9A-Fa-f]{40}$ ]]; then
  SIGN_HASH=$(echo "$IDENTITY" | tr 'a-f' 'A-F')
else
  SIGN_HASH=$(security find-identity -v -p codesigning | grep -F "\"$IDENTITY\"" | awk '{print $2}' | head -1 || true)
fi
if [ -z "$SIGN_HASH" ]; then
  echo "❌ 钥匙串里找不到签名证书「$IDENTITY」。" >&2
  exit 1
fi
# 证书本身（PEM）：按 SHA-1 在钥匙串里找，读 Team ID 和证书类型
CERT_PEM=$(security find-certificate -a -Z -p 2>/dev/null | awk -v hash="$SIGN_HASH" '
  /^SHA-1 hash:/ { take = ($3 == hash) }
  take { print }
  /END CERTIFICATE/ { if (take) exit }' | sed -n '/BEGIN CERTIFICATE/,/END CERTIFICATE/p' || true)
CERT_SUBJECT=""
if [ -n "$CERT_PEM" ]; then
  CERT_SUBJECT=$(echo "$CERT_PEM" | openssl x509 -noout -subject -nameopt multiline 2>/dev/null || true)
fi

TEAM_ID="${APPLE_TEAM_ID:-}"
if [ -z "$TEAM_ID" ]; then
  TEAM_ID=$(echo "$CERT_SUBJECT" | sed -nE 's/^ *organizationalUnitName *= *([A-Z0-9]+).*/\1/p' | head -1)
fi
if [ -z "$TEAM_ID" ]; then
  echo "❌ 读不出签名证书的 Team ID（证书 $SIGN_HASH），请设置 APPLE_TEAM_ID。" >&2
  exit 1
fi
APP_GROUP="$TEAM_ID.com.ccswitch.desktop"
VERSION=$(node -p 'require(process.argv[1]).version' "$ROOT/src-tauri/tauri.conf.json")
# 系统按扩展的 CFBundleVersion 缓存小组件清单（含配置项）：版本号不变时改了小组件也不会重读，
# 所以每次构建给一个新的 build 号。
# 用 Unix 秒：同一分钟内重编也会变，且是一个整数（CFBundleVersion 要求）
BUILD_NUMBER="${WIDGET_BUILD_NUMBER:-$(date +%s)}"

echo "▸ 签名身份: $IDENTITY（$SIGN_HASH）"
echo "▸ App Group: $APP_GROUP"
echo "▸ 版本: $VERSION  架构: $ARCHS"

# ── 编译 ───────────────────────────────────────────────────────────
# 用 Xcode 构建：app 扩展的入口 _NSExtensionMain、可配置小组件要的 Metadata.appintents
# （「编辑小组件」里的选项）都只有 Xcode 的构建流程会生成。工程由 gen_xcodeproj.py 按 Sources/、
# Resources/ 现生成，不进版本库；签名留给下面按自己的 entitlements 做。
rm -rf "$OUT"
mkdir -p "$OUT"
python3 "$SRC/sync_strings.py"
python3 "$SRC/sync_icons.py"
python3 "$SRC/gen_xcodeproj.py" "$OUT" >/dev/null
xcodebuild \
  -project "$OUT/CCSwitchWidget.xcodeproj" \
  -target CCSwitchWidget \
  -configuration Release \
  ARCHS="$ARCHS" \
  MARKETING_VERSION="$VERSION" \
  CURRENT_PROJECT_VERSION="$BUILD_NUMBER" \
  CCSWITCH_APP_GROUP="$APP_GROUP" \
  SYMROOT="$OUT/xcode" \
  OBJROOT="$OUT/xcode/obj" \
  -quiet build
ditto "$OUT/xcode/Release/CCSwitchWidget.appex" "$APPEX"
plutil -lint "$APPEX/Contents/Info.plist" >/dev/null
if [ ! -d "$APPEX/Contents/Resources/Metadata.appintents" ]; then
  echo "❌ 没有生成 Metadata.appintents，「编辑小组件」里不会有配置项" >&2
  exit 1
fi

cat > "$OUT/widget.entitlements" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>com.apple.security.app-sandbox</key>
  <true/>
  <key>com.apple.security.application-groups</key>
  <array>
    <string>$APP_GROUP</string>
  </array>
</dict>
</plist>
PLIST

# 主程序不进沙盒，只多一个 App Group：写快照进共享容器时 macOS 不会弹「访问其他 App 的数据」。
cat > "$OUT/app.entitlements" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>com.apple.security.application-groups</key>
  <array>
    <string>$APP_GROUP</string>
  </array>
</dict>
</plist>
PLIST

# ── 签名 ───────────────────────────────────────────────────────────
sign_args=(--force --options runtime --entitlements "$OUT/widget.entitlements" --sign "$SIGN_HASH")
# 公证要求带可信时间戳：按证书类型判断（身份写成 SHA-1 时名字里看不出来）
if echo "$CERT_SUBJECT$IDENTITY" | grep -q "Developer ID Application"; then
  sign_args+=(--timestamp)
fi
codesign "${sign_args[@]}" "$APPEX"
codesign --verify --strict --verbose=1 "$APPEX"

# ── Tauri 覆盖配置 ─────────────────────────────────────────────────
# 用 Python 写 JSON：路径里有引号、反斜杠也不会生成坏文件
python3 - "$APPEX" "$OUT/app.entitlements" "$OUT/tauri.widget.conf.json" <<'PY'
import json, sys
appex, entitlements, out = sys.argv[1:4]
config = {"bundle": {"macOS": {"files": {"PlugIns/CCSwitchWidget.appex": appex}, "entitlements": entitlements}}}
with open(out, "w") as fh:
    json.dump(config, fh, indent=2, ensure_ascii=False)
PY

echo "✅ $APPEX"
echo "   覆盖配置: $OUT/tauri.widget.conf.json"

if [ "$bundle" -eq 1 ]; then
  cd "$ROOT"
  APPLE_SIGNING_IDENTITY="$SIGN_HASH" pnpm tauri build --config "$OUT/tauri.widget.conf.json" "$@"
fi
