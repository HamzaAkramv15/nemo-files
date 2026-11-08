#!/usr/bin/env bash
# yami-files: build and install into ~/.local, no root needed. Needs Rust 1.89 or newer.
#   ./install.sh               build and install
#   ./install.sh --deps        what it needs from the system
#   ./install.sh --uninstall   remove it
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
PREFIX="${PREFIX:-$HOME/.local}"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
case "${1:-}" in
  --deps) echo "wayland, libxkbcommon; optional: ffmpegthumbnailer, libarchive, gvfs, udisks2"; exit 0 ;;
  --uninstall)
    rm -f "$PREFIX/bin/yami-files" "$DATA/applications/yami-files.desktop" "$DATA/xdg-desktop-portal/portals/yami.portal"
    rm -f "$DATA/dbus-1/services/org.freedesktop.impl.portal.desktop.yami.service" "$DATA/dbus-1/services/org.freedesktop.FileManager1.service"
    echo "yami-files removed"; exit 0 ;;
esac
cargo build --release --locked
install -Dm755 target/release/yami-files "$PREFIX/bin/yami-files"
install -Dm644 config/yami-files.desktop "$DATA/applications/yami-files.desktop"
install -Dm644 config/yami.portal "$DATA/xdg-desktop-portal/portals/yami.portal"
mkdir -p "$DATA/dbus-1/services"
for s in org.freedesktop.impl.portal.desktop.yami org.freedesktop.FileManager1; do
  sed "s|@BIN@|$PREFIX/bin/yami-files|" "config/$s.service" >"$DATA/dbus-1/services/$s.service"
done
echo "yami-files installed to $PREFIX/bin"
