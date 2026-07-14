#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:-0.0.0}"
ARCH="${2:-$(uname -m)}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
DIST="$ROOT/dist/macos"
STAGE="$DIST/stage"
BINARY_DIR="${BINARY_DIR:-$ROOT/target/release}"
DMG="$DIST/CodexPlusPlus-${VERSION}-macos-${ARCH}.dmg"
ICON_SOURCE="$ROOT/apps/codex-plus-manager/src-tauri/icons/icon.png"
ICON_NAME="codex-plus-plus.icns"
ICON_ICNS="$DIST/$ICON_NAME"

rm -rf "$DIST"
mkdir -p "$STAGE"

prepare_icon() {
  local iconset="$DIST/codex-plus-plus.iconset"
  rm -rf "$iconset"
  mkdir -p "$iconset"

  sips -z 16 16 "$ICON_SOURCE" --out "$iconset/icon_16x16.png" >/dev/null
  sips -z 32 32 "$ICON_SOURCE" --out "$iconset/icon_16x16@2x.png" >/dev/null
  sips -z 32 32 "$ICON_SOURCE" --out "$iconset/icon_32x32.png" >/dev/null
  sips -z 64 64 "$ICON_SOURCE" --out "$iconset/icon_32x32@2x.png" >/dev/null
  sips -z 128 128 "$ICON_SOURCE" --out "$iconset/icon_128x128.png" >/dev/null
  sips -z 256 256 "$ICON_SOURCE" --out "$iconset/icon_128x128@2x.png" >/dev/null
  sips -z 256 256 "$ICON_SOURCE" --out "$iconset/icon_256x256.png" >/dev/null
  sips -z 512 512 "$ICON_SOURCE" --out "$iconset/icon_256x256@2x.png" >/dev/null
  sips -z 512 512 "$ICON_SOURCE" --out "$iconset/icon_512x512.png" >/dev/null
  sips -z 1024 1024 "$ICON_SOURCE" --out "$iconset/icon_512x512@2x.png" >/dev/null

  iconutil -c icns "$iconset" -o "$ICON_ICNS"
}

create_app() {
  local app_name="$1"
  local executable_name="$2"
  local binary_path="$3"
  local bundle_id="$4"
  local lsui_element="${5:-false}"
  local app_dir="$STAGE/$app_name.app"

  if [ ! -x "$binary_path" ]; then
    echo "error: binary not found or not executable: $binary_path" >&2
    return 1
  fi

  rm -rf "$app_dir"
  mkdir -p "$app_dir/Contents/MacOS" "$app_dir/Contents/Resources"
  cp "$binary_path" "$app_dir/Contents/MacOS/$executable_name"
  cp "$ICON_ICNS" "$app_dir/Contents/Resources/$ICON_NAME"
  chmod +x "$app_dir/Contents/MacOS/$executable_name"
  printf 'APPL????' > "$app_dir/Contents/PkgInfo"
  cat > "$app_dir/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>$app_name</string>
  <key>CFBundleDisplayName</key>
  <string>$app_name</string>
  <key>CFBundleIdentifier</key>
  <string>$bundle_id</string>
  <key>CFBundleVersion</key>
  <string>$VERSION</string>
  <key>CFBundleShortVersionString</key>
  <string>$VERSION</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleSignature</key>
  <string>????</string>
  <key>CFBundleExecutable</key>
  <string>$executable_name</string>
  <key>CFBundleIconFile</key>
  <string>$ICON_NAME</string>
  <key>LSMinimumSystemVersion</key>
  <string>12.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>LSUIElement</key>
  <$lsui_element/>
</dict>
</plist>
PLIST
}

sign_app() {
  local app_dir="$1"
  local executable
  executable="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app_dir/Contents/Info.plist")"
  codesign --force --sign - "$app_dir/Contents/MacOS/$executable"
  codesign --force --sign - "$app_dir"
}

verify_app() {
  local app_dir="$1"
  local plist="$app_dir/Contents/Info.plist"
  local plutil_bin
  plutil_bin="$(command -v plutil || true)"
  if [ -n "$plutil_bin" ]; then
    "$plutil_bin" -lint "$plist" >/dev/null
  else
    /usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$plist" >/dev/null
  fi
  if [ ! -f "$app_dir/Contents/PkgInfo" ]; then
    echo "error: missing PkgInfo in $app_dir" >&2
    return 1
  fi
  codesign -dv "$app_dir" >/dev/null 2>&1 || {
    echo "error: codesign verification failed for $app_dir" >&2
    return 1
  }
}

prepare_icon
create_app "Codex++" "CodexPlusPlus" "$BINARY_DIR/codex-plus-plus" "com.bigpizzav3.codexplusplus" "true"
create_app "Codex++ 管理工具" "CodexPlusPlusManager" "$BINARY_DIR/codex-plus-plus-manager" "com.bigpizzav3.codexplusplus.manager" "false"

sign_app "$STAGE/Codex++.app"
sign_app "$STAGE/Codex++ 管理工具.app"

verify_app "$STAGE/Codex++.app"
verify_app "$STAGE/Codex++ 管理工具.app"

ln -s /Applications "$STAGE/Applications"

hdiutil create -volname "Codex++" -srcfolder "$STAGE" -ov -format UDZO "$DMG"
echo "$DMG"
