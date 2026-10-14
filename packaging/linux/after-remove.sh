#!/bin/sh
# Debian package post-remove.
set -e
rm -f /usr/bin/sherpad /usr/bin/sherpa
rm -f /usr/lib/udev/rules.d/70-sherpa.rules /usr/lib/udev/rules.d/71-sherpa-uinput.rules
rm -f /usr/lib/systemd/user/sherpad.service
if command -v udevadm >/dev/null 2>&1; then
  udevadm control --reload-rules || true
fi
exit 0
