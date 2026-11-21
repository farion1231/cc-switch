#!/usr/bin/env bash
# 打开一个窗口看小组件的 Liquid Glass 效果（需要 macOS 26）：铺当前桌面壁纸，左浅色右深色。
# 玻璃材质要实时合成，ImageRenderer（preview.sh）画不出来，所以这里是个真窗口。
# 用法：macos-widget/glass-preview.sh [widget-snapshot.json]（不传用示例数据）
#       macos-widget/glass-preview.sh --capture <clear|light|dark> [snapshot.json]
#         不弹窗口给人看：等玻璃合成好截窗口存成 src-tauri/target/macos-widget/glass-<外观>.png 就退出
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/src-tauri/target/macos-widget"
APP="$OUT/CCSwitchWidgetGlassPreview.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
python3 "$ROOT/macos-widget/sync_strings.py"
# 预览程序没有 lproj：文案从这里按系统语言取（AuthCenterText.swift）
export CCSW_RESOURCES="$ROOT/macos-widget/Resources"
xcrun --sdk macosx swiftc -D CC_SWITCH_WIDGET_PREVIEW -target "$(uname -m)-apple-macos14.0" \
  -framework SwiftUI -framework WidgetKit -framework Charts -framework AppKit \
  "$ROOT"/macos-widget/Sources/*.swift "$ROOT/macos-widget/Preview/Glass/main.swift" \
  -o "$APP/Contents/MacOS/GlassPreview"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleIdentifier</key><string>com.ccswitch.widget-glass-preview</string>
  <key>CFBundleName</key><string>CC Switch Widget Preview</string>
  <key>CFBundleExecutable</key><string>GlassPreview</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
codesign --force --sign - "$APP" >/dev/null 2>&1
# 没给 JSON 时去读主程序写的真实快照：App Group 名字取自 build.sh 编出的 appex
GROUP=""
if [ -f "$OUT/CCSwitchWidget.appex/Contents/Info.plist" ]; then
  GROUP=$(/usr/libexec/PlistBuddy -c "Print :CCSwitchAppGroup" "$OUT/CCSwitchWidget.appex/Contents/Info.plist" 2>/dev/null || true)
fi
export CC_SWITCH_WIDGET_APP_GROUP="$GROUP"
if [ "${1:-}" = "--capture" ]; then
  look="${2:-panelLight}"
  shift 2 || shift $#
  CCSW_LOOK="$look" CCSW_CAPTURE="$OUT/glass-$look.png" "$APP/Contents/MacOS/GlassPreview" "$@"
  exit 0
fi
if [ $# -gt 0 ]; then
  open -n --env "CC_SWITCH_WIDGET_APP_GROUP=$GROUP" --env "CCSW_RESOURCES=$CCSW_RESOURCES" "$APP" --args "$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
else
  open -n --env "CC_SWITCH_WIDGET_APP_GROUP=$GROUP" --env "CCSW_RESOURCES=$CCSW_RESOURCES" "$APP"
fi
