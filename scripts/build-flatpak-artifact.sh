#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${ROOT_DIR}/dist-flatpak"
ARTIFACT="${OUT_DIR}/CC-Switch-Linux.flatpak"
FLATPAK_IMAGE="${FLATPAK_IMAGE:-cc-switch-flatpak-build:local}"
FLATPAK_DOCKERFILE="${ROOT_DIR}/scripts/docker/flatpak-ubuntu22.Dockerfile"
DEB_SRC="$(find "${ROOT_DIR}/src-tauri/target/release/bundle/deb" -type f -name '*.deb' | head -n 1 || true)"
DEB_REL=""

# Extract runtime version from manifest
RUNTIME_VERSION="$(grep 'runtime-version:' "${ROOT_DIR}/flatpak/com.ccswitch.desktop.yml" | awk '{print $2}' | tr -d "'")"

if ! command -v docker >/dev/null 2>&1; then
  echo "docker is required but not found" >&2
  exit 1
fi

if ! docker info >/dev/null 2>&1; then
  echo "docker daemon is not available for current user" >&2
  exit 1
fi

if [[ -z "${DEB_SRC}" ]]; then
  "${ROOT_DIR}/scripts/ubuntu22-build-env.sh" build-deb
  DEB_SRC="$(find "${ROOT_DIR}/src-tauri/target/release/bundle/deb" -type f -name '*.deb' | head -n 1 || true)"
fi

if [[ -z "${DEB_SRC}" ]]; then
  echo "Unable to locate generated .deb bundle" >&2
  exit 1
fi

DEB_REL="${DEB_SRC#"${ROOT_DIR}/"}"

mkdir -p "${OUT_DIR}" "${ROOT_DIR}/.cache/flatpak-system"

docker build \
  --build-arg UID="$(id -u)" \
  --build-arg GID="$(id -g)" \
  --tag "${FLATPAK_IMAGE}" \
  --file "${FLATPAK_DOCKERFILE}" \
  "${ROOT_DIR}"

docker run --rm -t --privileged -u 0:0 \
  -v "${ROOT_DIR}:/workspace" \
  -v "${ROOT_DIR}/.cache/flatpak-system:/var/lib/flatpak" \
  -w /workspace \
  "${FLATPAK_IMAGE}" \
  bash -lc 'set -euo pipefail
    mkdir -p /run/dbus
    if [[ ! -S /run/dbus/system_bus_socket ]]; then
      dbus-daemon --system --fork --nopidfile
    fi

    cp -f "/workspace/'"${DEB_REL}"'" flatpak/cc-switch.deb

    flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
    flatpak install --noninteractive -y flathub org.gnome.Platform//'"${RUNTIME_VERSION}"' org.gnome.Sdk//'"${RUNTIME_VERSION}"'

    rm -rf manual-deb-extract manual-flatpak-build manual-flatpak-repo
    mkdir -p manual-flatpak-build manual-deb-extract

    flatpak build-init manual-flatpak-build com.ccswitch.desktop org.gnome.Sdk org.gnome.Platform '"${RUNTIME_VERSION}"'

    cp -f "/workspace/'"${DEB_REL}"'" manual-deb-extract/cc-switch.deb
    cd manual-deb-extract
    ar -x cc-switch.deb
    tar -xf data.tar.*
    cp -a usr/* /workspace/manual-flatpak-build/files/

    # The Tauri tray backend dlopens these at runtime; bundle them explicitly so
    # the Flatpak does not depend on host packages.
    mkdir -p /workspace/manual-flatpak-build/files/lib
    shopt -s nullglob
    for lib in \
      /usr/lib/x86_64-linux-gnu/libayatana-appindicator3.so* \
      /usr/lib/x86_64-linux-gnu/libappindicator3.so* \
      /usr/lib/x86_64-linux-gnu/libayatana-indicator3.so* \
      /usr/lib/x86_64-linux-gnu/libayatana-ido3*.so* \
      /usr/lib/x86_64-linux-gnu/libdbusmenu-gtk3.so* \
      /usr/lib/x86_64-linux-gnu/libdbusmenu-glib.so* \
      /usr/lib/x86_64-linux-gnu/libdbusmenu-gtk.so*; do
      cp -a "${lib}" /workspace/manual-flatpak-build/files/lib/
    done
    cd /workspace

    rm -f manual-flatpak-build/files/share/applications/*.desktop
    install -Dm644 flatpak/com.ccswitch.desktop.desktop manual-flatpak-build/files/share/applications/com.ccswitch.desktop.desktop
    install -Dm644 flatpak/com.ccswitch.desktop.metainfo.xml manual-flatpak-build/files/share/metainfo/com.ccswitch.desktop.metainfo.xml
    install -Dm644 src-tauri/icons/128x128.png manual-flatpak-build/files/share/icons/hicolor/128x128/apps/com.ccswitch.desktop.png

    flatpak build-finish manual-flatpak-build \
      --share=ipc --share=network --socket=wayland --socket=fallback-x11 --device=dri \
      --talk-name=org.kde.StatusNotifierWatcher --filesystem=xdg-run/tray-icon:create --filesystem=home

    flatpak build-export manual-flatpak-repo manual-flatpak-build
    flatpak build-bundle --runtime-repo=https://flathub.org/repo/flathub.flatpakrepo manual-flatpak-repo dist-flatpak/CC-Switch-Linux.flatpak com.ccswitch.desktop

    dbus-run-session -- bash -lc "flatpak uninstall -y com.ccswitch.desktop >/dev/null 2>&1 || true; flatpak install --noninteractive -y /workspace/dist-flatpak/CC-Switch-Linux.flatpak; flatpak run --command=sh com.ccswitch.desktop -c '\''echo smoke-ok'\''"

    rm -rf manual-deb-extract manual-flatpak-build manual-flatpak-repo
  '

docker run --rm -u 0:0 \
  -v "${ROOT_DIR}:/workspace" \
  -w /workspace \
  "${FLATPAK_IMAGE}" \
  bash -lc "chown -R $(id -u):$(id -g) dist-flatpak flatpak/cc-switch.deb"

echo "Flatpak artifact: ${ARTIFACT}"
