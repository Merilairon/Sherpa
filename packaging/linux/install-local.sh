#!/bin/sh
# Install the daemon, udev rules and the systemd user unit from a source build or from
# an AppImage/tar.gz release. Run as your user; it asks for sudo for the udev rules.
#
#   sh packaging/linux/install-local.sh [path/to/basecampd]
set -e
HERE="$(cd "$(dirname "$0")" && pwd)"
BIN="${1:-$HERE/../../target/release/basecampd}"
[ -x "$BIN" ] || BIN="$HERE/../bin/basecampd"
if [ ! -x "$BIN" ]; then
  echo "basecampd binary not found; build it with: cargo build --release -p basecampd" >&2
  exit 1
fi
install -Dm755 "$BIN" "$HOME/.local/bin/basecampd"
mkdir -p "$HOME/.config/systemd/user"
sed "s|^ExecStart=.*|ExecStart=$HOME/.local/bin/basecampd|" "$HERE/basecampd.service" > "$HOME/.config/systemd/user/basecampd.service"
echo "Installing udev rules (sudo)…"
sudo install -Dm644 "$HERE/70-basecamp.rules" /etc/udev/rules.d/70-basecamp.rules
printf "Also allow synthetic input (needed for host-executed shortcuts and DisplayPad)? [y/N] "
read -r ans
if [ "$ans" = "y" ] || [ "$ans" = "Y" ]; then
  sudo install -Dm644 "$HERE/71-basecamp-uinput.rules" /etc/udev/rules.d/71-basecamp-uinput.rules
fi
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=hidraw --action=change
systemctl --user daemon-reload
systemctl --user enable --now basecampd.service
echo "Done. Re-plug your device if it was connected. Status: systemctl --user status basecampd"
