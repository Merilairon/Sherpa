#!/bin/sh
# Install the daemon, udev rules and the systemd user unit from a source build or from
# an AppImage/tar.gz release. Run as your user; it asks for sudo for the udev rules.
#
#   sh packaging/linux/install-local.sh [path/to/sherpad]
set -e
HERE="$(cd "$(dirname "$0")" && pwd)"
BIN="${1:-$HERE/../../target/release/sherpad}"
[ -x "$BIN" ] || BIN="$HERE/../bin/sherpad"
if [ ! -x "$BIN" ]; then
  echo "sherpad binary not found; build it with: cargo build --release -p sherpad" >&2
  exit 1
fi
install -Dm755 "$BIN" "$HOME/.local/bin/sherpad"
mkdir -p "$HOME/.config/systemd/user"
sed "s|^ExecStart=.*|ExecStart=$HOME/.local/bin/sherpad|" "$HERE/sherpad.service" > "$HOME/.config/systemd/user/sherpad.service"
echo "Installing udev rules (sudo)…"
sudo install -Dm644 "$HERE/70-sherpa.rules" /etc/udev/rules.d/70-sherpa.rules
printf "Also allow synthetic input (needed for host-executed shortcuts and DisplayPad)? [y/N] "
read -r ans
if [ "$ans" = "y" ] || [ "$ans" = "Y" ]; then
  sudo install -Dm644 "$HERE/71-sherpa-uinput.rules" /etc/udev/rules.d/71-sherpa-uinput.rules
fi
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=hidraw --action=change
systemctl --user daemon-reload
systemctl --user enable --now sherpad.service
echo "Done. Re-plug your device if it was connected. Status: systemctl --user status sherpad"
