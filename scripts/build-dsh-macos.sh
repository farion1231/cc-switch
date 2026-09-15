#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$project_root"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This build script requires macOS." >&2
  exit 1
fi
if [[ "$(uname -m)" != "arm64" ]]; then
  echo "This build script currently produces the Apple Silicon package only." >&2
  exit 1
fi
if [[ ! -x node_modules/.bin/vite || ! -x node_modules/.bin/tauri ]]; then
  echo "Frontend dependencies are missing. Run: pnpm install --frozen-lockfile" >&2
  exit 1
fi

version="$(node -p "require('./package.json').version")"
build_dir="$project_root/work/release-$version"
config_file="$project_root/scripts/tauri-build.macos.json"
stage_dir="$build_dir/dmg-root"
release_dir="$project_root/release"
app_path="$project_root/src-tauri/target/release/bundle/macos/CC Switch.app"
dmg_path="$release_dir/CC-Switch-$version-DSH-Codex-arm64.dmg"

mkdir -p "$build_dir" "$stage_dir" "$release_dir"

./node_modules/.bin/vite build
CARGO_PROFILE_RELEASE_STRIP=false \
CARGO_BUILD_JOBS=1 \
CARGO_BUILD_PIPELINING=false \
  ./node_modules/.bin/tauri build --config "$config_file"

codesign --force --deep --sign - --timestamp=none "$app_path"
codesign --verify --deep --strict --verbose=2 "$app_path"

ditto "$app_path" "$stage_dir/CC Switch.app"
if [[ ! -L "$stage_dir/Applications" ]]; then
  ln -s /Applications "$stage_dir/Applications"
fi
hdiutil create \
  -volname "CC Switch DSH Codex $version" \
  -srcfolder "$stage_dir" \
  -ov \
  -format UDZO \
  "$dmg_path"

mount_point="$(hdiutil attach -readonly -nobrowse -noverify "$dmg_path" | awk '/\/Volumes\// {sub(/^.*\/Volumes\//,"/Volumes/"); print; exit}')"
mounted_app="$mount_point/CC Switch.app"
/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$mounted_app/Contents/Info.plist"
file "$mounted_app/Contents/MacOS/cc-switch"
codesign --verify --deep --strict --verbose=2 "$mounted_app"
hdiutil detach "$mount_point"

shasum -a 256 "$dmg_path"
echo "$dmg_path"
