#!/usr/bin/env bash
# Build, sign, notarize and package CC Switch Light for macOS.
# Leaves upload-ready artifacts in src-tauri/target/light-release/<tag>/.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

IDENTITY="${APPLE_SIGNING_IDENTITY:-Developer ID Application: Guofeng Liu (U8U443D7ZL)}"
KEY="${TAURI_SIGNING_PRIVATE_KEY_PATH:-$HOME/.tauri/cc-switch-light.key}"
PROFILE="${NOTARY_PROFILE:-agentspace-notary}"
APP_NAME="CC Switch Light"

VERSION="$(python3 -c 'import json;print(json.load(open("src-tauri/tauri.conf.json"))["version"])')"
TAG="v$VERSION"
APP="$ROOT/src-tauri/target/universal-apple-darwin/release/bundle/macos/$APP_NAME.app"
OUT="$ROOT/src-tauri/target/light-release/$TAG"
DMG="$OUT/CC-Switch-Light-$TAG-macOS.dmg"
ZIP="$OUT/CC-Switch-Light-$TAG-macOS.zip"
TARBALL="$OUT/CC-Switch-Light-$TAG-macOS.tar.gz"

fail() { echo "ERROR: $*" >&2; exit 1; }

[ -z "$(git status --porcelain)" ] || fail "working tree is dirty; commit before releasing"
[ -f "$KEY" ] || fail "missing updater private key: $KEY"
[ -f "$KEY.pub" ] || fail "missing updater public key: $KEY.pub"
security find-identity -v -p codesigning | grep -Fq "$IDENTITY" || fail "signing identity not found: $IDENTITY"

python3 - "$KEY.pub" <<'PY' || fail "updater pubkey in tauri.conf.json does not match $KEY.pub"
import base64, json, sys
conf = json.load(open("src-tauri/tauri.conf.json"))
embedded = base64.b64decode(conf["plugins"]["updater"]["pubkey"]).decode().strip()
actual = base64.b64decode(open(sys.argv[1]).read()).decode().strip()
sys.exit(0 if embedded == actual else 1)
PY

if [ -z "${SDKROOT:-}" ] || [ ! -d "${SDKROOT:-}" ]; then
  SDKROOT="$(ls -d /Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.Platform/Developer/SDKs/MacOSX*.sdk 2>/dev/null | sort -V | tail -1)"
fi
[ -d "${SDKROOT:-}" ] || fail "no Xcode macOS SDK found; set SDKROOT"
export SDKROOT
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-12.0}"

notarize() {
  local file="$1" json status attempt
  for attempt in 1 2 3; do
    if json="$(xcrun notarytool submit "$file" --keychain-profile "$PROFILE" --wait --output-format json 2>&1)"; then
      status="$(python3 -c 'import json,sys;print(json.load(sys.stdin).get("status",""))' <<<"$json" 2>/dev/null || true)"
      if [ "$status" = "Accepted" ]; then
        echo "notary: Accepted $(basename "$file")"
        return 0
      fi
      echo "notary: status ${status:-unknown} for $(basename "$file") (attempt $attempt)" >&2
    else
      echo "notary: submit failed for $(basename "$file") (attempt $attempt)" >&2
      echo "$json" | tail -5 >&2
    fi
    sleep 20
  done
  return 1
}

staple() {
  local target="$1" attempt
  for attempt in 1 2 3; do
    if xcrun stapler staple "$target" && xcrun stapler validate "$target"; then
      return 0
    fi
    echo "stapler: retrying $(basename "$target") (attempt $attempt)" >&2
    sleep 10
  done
  return 1
}

echo "==> $APP_NAME $TAG (universal), SDK $SDKROOT"
npx -y pnpm@10.12.3 install --frozen-lockfile
APPLE_SIGNING_IDENTITY="$IDENTITY" \
TAURI_SIGNING_PRIVATE_KEY="$KEY" \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
npx -y pnpm@10.12.3 tauri build --target universal-apple-darwin --bundles app

[ -d "$APP" ] || fail "app not found: $APP"
BIN_NAME="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$APP/Contents/Info.plist")"
ARCHS="$(lipo -archs "$APP/Contents/MacOS/$BIN_NAME")"
case "$ARCHS" in
  *arm64*x86_64*|*x86_64*arm64*) ;;
  *) fail "expected a universal binary, got: $ARCHS" ;;
esac
codesign --verify --deep --strict --verbose=2 "$APP"
SIGN_INFO="$(codesign -dv --verbose=4 "$APP" 2>&1)"
grep -Fq "Authority=$IDENTITY" <<<"$SIGN_INFO" || fail "app is not signed by $IDENTITY"
# Tauri passes --options runtime to executables only; sign the bundle itself with
# runtime + timestamp so both layers carry the flag notarization expects.
if ! grep -q 'Runtime Version' <<<"$SIGN_INFO"; then
  codesign --force --timestamp --options runtime --sign "$IDENTITY" "$APP"
  SIGN_INFO="$(codesign -dv --verbose=4 "$APP" 2>&1)"
fi
codesign --verify --deep --strict --verbose=2 "$APP"
# grep under `pipefail` would make codesign die of SIGPIPE and fail the pipeline.
grep -q 'Runtime Version' <<<"$SIGN_INFO" || fail "hardened runtime missing from app signature"
EXE_INFO="$(codesign -dv --verbose=4 "$APP/Contents/MacOS/$BIN_NAME" 2>&1)"
grep -q 'Runtime Version' <<<"$EXE_INFO" || fail "hardened runtime missing from the main executable"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "==> Notarizing app"
ditto -c -k --keepParent "$APP" "$TMP/app.zip"
notarize "$TMP/app.zip" || fail "app notarization failed"
staple "$APP" || fail "could not staple the app"
spctl --assess --type execute -vv "$APP"

mkdir -p "$OUT"

echo "==> Creating DMG"
STAGE="$TMP/dmg"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "$APP_NAME" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
codesign --force --timestamp --sign "$IDENTITY" "$DMG"
notarize "$DMG" || fail "DMG notarization failed"
staple "$DMG" || fail "could not staple the DMG"
spctl --assess --type open --context context:primary-signature -vv "$DMG"

echo "==> Creating zip and updater tarball"
ditto -c -k --keepParent "$APP" "$ZIP"
( cd "$(dirname "$APP")" && COPYFILE_DISABLE=1 tar -czf "$TARBALL" "$APP_NAME.app" )
npx -y pnpm@10.12.3 tauri signer sign -f "$KEY" -p "" "$TARBALL"
[ -s "$TARBALL.sig" ] || fail "updater signature missing: $TARBALL.sig"

echo "==> Writing latest.json and checksums"
python3 - "$VERSION" "$TAG" "$TARBALL.sig" "$OUT/latest.json" <<'PY'
import datetime, json, sys
version, tag, sig_path, out = sys.argv[1:5]
sig = open(sig_path).read().strip()
url = f"https://github.com/misswell/cc-switch/releases/download/{tag}/CC-Switch-Light-{tag}-macOS.tar.gz"
data = {
    "version": version,
    "notes": f"CC Switch Light {tag}",
    "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "platforms": {
        "darwin-aarch64": {"signature": sig, "url": url},
        "darwin-x86_64": {"signature": sig, "url": url},
    },
}
json.dump(data, open(out, "w"), ensure_ascii=False, indent=2)
PY
( cd "$OUT" && shasum -a 256 "$(basename "$DMG")" "$(basename "$ZIP")" "$(basename "$TARBALL")" > SHA256SUMS.txt )

echo
echo "TAG=$TAG"
echo "RELEASE_DIR=$OUT"
ls -lh "$OUT"
