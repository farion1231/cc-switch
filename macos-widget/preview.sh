#!/usr/bin/env bash
# 把中尺寸小组件渲染成 PNG，改界面时不用打包安装就能看效果。
# 用法：macos-widget/preview.sh [widget-snapshot.json]（不传用示例数据）
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/src-tauri/target/macos-widget"
mkdir -p "$OUT"
python3 "$ROOT/macos-widget/sync_strings.py"
# 预览程序没有 lproj：文案从这里按系统语言取（AuthCenterText.swift）
export CCSW_RESOURCES="$ROOT/macos-widget/Resources"
xcrun --sdk macosx swiftc -D CC_SWITCH_WIDGET_PREVIEW -target "$(uname -m)-apple-macos14.0" \
  -framework SwiftUI -framework WidgetKit -framework Charts -framework AppKit \
  "$ROOT"/macos-widget/Sources/*.swift "$ROOT/macos-widget/Preview/main.swift" \
  -o "$OUT/widget-preview"
"$OUT/widget-preview" "$OUT" "$@"
