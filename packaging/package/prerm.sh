#!/bin/sh
# Package pre-removal (.deb prerm and .rpm %preun): when the package is
# removed (not upgraded), removes the network component it installed. A
# component installed by the app (AppImage) or by hand is left alone.
#
# It runs the installer copied into the installation, owned by root, and
# never fails the package transaction.
#
# $1: "remove" (deb; "upgrade" and others are ignored); 0 (rpm erase; 1 is
# an upgrade).
set -u

prefix=/opt/ikebox

case "${1:-}" in
remove | 0) ;;
*) exit 0 ;;
esac

[ "$(cat "$prefix/ORIGIN" 2>/dev/null)" = package ] || exit 0
if [ -f "$prefix/libexec/install-native.sh" ]; then
    bash "$prefix/libexec/install-native.sh" uninstall ||
        echo "ikebox: warning: the network component could not be removed;" \
            "run: sudo $prefix/libexec/install-native.sh uninstall" >&2
fi
exit 0
