#!/bin/sh
# Debian package post-install: daemon on PATH, udev rules, systemd user unit.
set -e
APP="/opt/Sherpa"
RES="$APP/resources"
ln -sf "$RES/bin/sherpad" /usr/bin/sherpad
if [ -f "$APP/sherpa" ]; then
  ln -sf "$APP/sherpa" /usr/bin/sherpa
fi
install -Dm644 "$RES/linux/70-sherpa.rules" /usr/lib/udev/rules.d/70-sherpa.rules
install -Dm644 "$RES/linux/71-sherpa-uinput.rules" /usr/lib/udev/rules.d/71-sherpa-uinput.rules
install -Dm644 "$RES/linux/sherpad.service" /usr/lib/systemd/user/sherpad.service
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
