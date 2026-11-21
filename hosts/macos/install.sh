#!/bin/bash
# Install buttre (Buttre.app next to this script) for every account on this
# Mac, then put Telex, VNI and Chữ Nôm straight into the input menu.
#
# Run as YOUR user, not with sudo: the copy into /Library asks for the admin
# password itself, while enabling input sources must happen in the logged-in
# user's session. The bundled ethnic-minority keyboards are installed too;
# add them when needed under System Settings → Keyboard → Text Input → Edit
# → (+) → Vietnamese.
set -euo pipefail

if [ "$(id -u)" -eq 0 ]; then
    echo "Run ./install.sh as your own user (not sudo)." >&2
    exit 1
fi

SRC="$(cd "$(dirname "$0")" && pwd)/Buttre.app"
DEST="/Library/Input Methods/Buttre.app"
[ -d "$SRC" ] || { echo "Buttre.app not found next to install.sh" >&2; exit 1; }

# An older copy in the per-user folder would shadow/duplicate this one.
if [ -d "$HOME/Library/Input Methods/Buttre.app" ]; then
    echo "==> Removing old per-user copy"
    rm -rf "$HOME/Library/Input Methods/Buttre.app"
fi

echo "==> Copying Buttre.app to /Library/Input Methods (admin password)"
# Stop a running older version so the next keystroke loads the new one.
pkill -f "$DEST/Contents/MacOS/buttre" 2>/dev/null || true
sudo rm -rf "$DEST"
sudo ditto "$SRC" "$DEST"
# The session's imklaunchagent keeps state for the bundle just deleted and
# fails to launch the new copy (buttre in the menu, typing nothing) until
# logout. launchd restarts it at once with the new bundle.
launchctl kickstart -k "gui/$(id -u)/com.apple.imklaunchagent" || true

echo "==> Enabling buttre Telex, VNI, Chữ Nôm"
"$DEST/Contents/MacOS/buttre" --install

echo "Done. Pick buttre from the input menu in the menu bar (or Ctrl+Space)."
