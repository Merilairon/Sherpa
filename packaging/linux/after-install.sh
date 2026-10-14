#!/bin/sh
# Debian package post-install: daemon on PATH, udev rules, systemd user unit.
set -e
APP="/opt/Base Camp Linux"
RES="$APP/resources"
ln -sf "$RES/bin/basecampd" /usr/bin/basecampd
if [ -f "$APP/basecamp-linux" ]; then
  ln -sf "$APP/basecamp-linux" /usr/bin/basecamp-linux
fi
install -Dm644 "$RES/linux/70-basecamp.rules" /usr/lib/udev/rules.d/70-basecamp.rules
install -Dm644 "$RES/linux/71-basecamp-uinput.rules" /usr/lib/udev/rules.d/71-basecamp-uinput.rules
install -Dm644 "$RES/linux/basecampd.service" /usr/lib/systemd/user/basecampd.service
if command -v udevadm >/dev/null 2>&1; then
  udevadm control --reload-rules || true
  udevadm trigger --subsystem-match=hidraw --action=change || true
  udevadm trigger --subsystem-match=misc --sysname-match=uinput --action=change || true
fi
# Chromium sandbox helper permissions (electron-builder default).
if [ -f "$APP/chrome-sandbox" ]; then
  chmod 4755 "$APP/chrome-sandbox" || true
fi
exit 0
