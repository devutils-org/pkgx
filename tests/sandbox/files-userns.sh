#!/bin/sh
# Real managed-file reconciliation (ADR 013) in a disposable sandbox: the host is
# bound read-only, /tmp (with HOME and the state directory) is a private tmpfs and
# the network namespace has only loopback. No package manager is invoked.
#
# Usage: tests/sandbox/files-userns.sh PKGX_BINARY
set -eu
PKGX=${1:?usage: files-userns.sh PKGX_BINARY}
HERE=$(cd "$(dirname "$0")" && pwd)
[ "$(id -u)" -ne 0 ] || { echo 'refusing to run as real root' >&2; exit 1; }
exec bwrap --unshare-user --unshare-pid --unshare-ipc --unshare-net \
    --die-with-parent --ro-bind / / --dev /dev --proc /proc --tmpfs /tmp \
    --ro-bind "$PKGX" /tmp/bin/pkgx --ro-bind "$HERE/files-checks.sh" /tmp/checks.sh \
    env -i PATH=/tmp/bin:/usr/bin LC_ALL=C sh /tmp/checks.sh
