#!/bin/sh
# Debian package post-remove.
set -e
rm -f /usr/bin/basecampd /usr/bin/basecamp-linux
rm -f /usr/lib/udev/rules.d/70-basecamp.rules /usr/lib/udev/rules.d/71-basecamp-uinput.rules
rm -f /usr/lib/systemd/user/basecampd.service
if command -v udevadm >/dev/null 2>&1; then
  udevadm control --reload-rules || true
fi
exit 0
