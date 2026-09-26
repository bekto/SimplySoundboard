#!/bin/sh
# Package post-install hook: refresh udev so the shipped keyboard rule (which grants
# the logged-in user access to /dev/input for global hotkeys) applies immediately,
# without a logout. Never fails the installation.
udevadm control --reload 2>/dev/null || true
udevadm trigger --subsystem-match=input --action=change 2>/dev/null || true
exit 0
