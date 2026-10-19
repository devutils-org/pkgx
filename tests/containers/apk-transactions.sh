#!/bin/sh
# NEVER run on a workstation. This mutates only a disposable Alpine guest.
set -eu
installed() { apk info -e "$1" >/dev/null 2>&1; }
in_world() { grep -qx "$1" /etc/apk/world; }
fail() { echo "$1" >&2; exit 1; }
! installed tree || fail 'tree must be absent before the check'
cat > /tmp/tree.toml <<'MANIFEST'
version = 1
[packages.apk]
install = ['tree']
MANIFEST
xpac apply -f /tmp/tree.toml --dry-run --json
if printf 'no\n' | xpac apply -f /tmp/tree.toml; then
    fail 'unconfirmed apply unexpectedly succeeded'
fi
! installed tree || fail 'unconfirmed apply changed the system'
xpac apply -f /tmp/tree.toml --yes --state-dir /tmp/xpac-records --json
installed tree && in_world tree || fail 'requested package is not installed in WORLD'
xpac apply -f /tmp/tree.toml --yes --json > /tmp/second-apply.json
# Repeated application is a verified no-op.
grep -q '"operations":\[\]' /tmp/second-apply.json
# apk would edit WORLD and remove nothing: refused before it can.
if xpac remove apk:musl --dry-run; then
    fail 'removal of a needed package unexpectedly previewed'
fi
cp /etc/apk/world /tmp/world.before
# A reviewed removal plan becomes stale when the inventory changes.
xpac plan apk:tree --remove --save /tmp/stale.json --json
xpac install apk:strace --yes --state-dir /tmp/xpac-records --json
installed strace && installed libdw && ! in_world libdw || fail 'strace did not bring libdw as a dependency'
if xpac apply --plan /tmp/stale.json --yes --json; then
    fail 'stale saved plan unexpectedly executed'
fi
installed tree || fail 'stale plan changed the system'
xpac plan apk:tree --remove --save /tmp/review.json --expires-in 300 --json
xpac apply --plan /tmp/review.json --yes --state-dir /tmp/xpac-records --json
! installed tree && ! in_world tree || fail 'reviewed removal left the package'
# A dependency asked for is recorded in WORLD, not reinstalled.
before=$(apk list --installed libdw)
xpac install apk:libdw --yes --state-dir /tmp/xpac-records --json
in_world libdw && [ "$(apk list --installed libdw)" = "$before" ] || fail 'libdw was not marked explicit in place'
xpac remove apk:strace --yes --state-dir /tmp/xpac-records --json
! installed strace && installed libdw || fail 'removal took the explicit dependency with it'
xpac history --state-dir /tmp/xpac-records --json > /tmp/history.json
[ "$(grep -o '"success":true' /tmp/history.json | wc -l)" -ge 5 ] || fail 'journal is missing results'
xpac doctor --state-dir /tmp/xpac-records --json
