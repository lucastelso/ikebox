#!/bin/sh
# Package post-install (.deb postinst and .rpm %post): installs or updates
# the network component from the bundle the package ships, owned by root
# (ADR-0018, origin "package"). The app does not ask for the administrator
# password then, and an update of the package updates the component.
#
# It never fails the package transaction: if the component cannot be
# installed now, the app offers to install it, as with the AppImage.
#
# $1: "configure" (deb, install and upgrade); 1 or 2 (rpm, install/upgrade).
set -u

bundle=/usr/lib/ipsec-client/network
prefix=/opt/ipsec-client

case "${1:-}" in
configure | 1 | 2) ;;
*) exit 0 ;;
esac

origin=$(cat "$prefix/ORIGIN" 2>/dev/null || echo none)
if [ "$origin" = manual ]; then
    echo "ipsec-client: the network component was installed by hand; left as it is"
    exit 0
fi

if ! bash "$bundle/install-native.sh" install --bundle "$bundle" --origin package; then
    echo "ipsec-client: warning: the network component was not installed;" \
        "open IPsec Client to install it" >&2
fi
exit 0
