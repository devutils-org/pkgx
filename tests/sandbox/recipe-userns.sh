#!/bin/sh
# Real recipe install/uninstall in a disposable sandbox: the host is bound
# read-only, /tmp and HOME are private tmpfs, and the network namespace has only
# loopback. The only script executed is the fixture written by recipe-checks.sh.
#
# Usage: tests/sandbox/recipe-userns.sh PKGX_BINARY
set -eu
PKGX=${1:?usage: recipe-userns.sh PKGX_BINARY}
HERE=$(cd "$(dirname "$0")" && pwd)
[ "$(id -u)" -ne 0 ] || { echo 'refusing to run as real root' >&2; exit 1; }
exec bwrap --unshare-user --unshare-pid --unshare-ipc --unshare-net \
    --die-with-parent --ro-bind / / --dev /dev --proc /proc --tmpfs /tmp \
    --ro-bind "$PKGX" /tmp/bin/pkgx --ro-bind "$HERE/recipe-checks.sh" /tmp/checks.sh \
    env -i PATH=/tmp/bin:/usr/bin LC_ALL=C sh /tmp/checks.sh
