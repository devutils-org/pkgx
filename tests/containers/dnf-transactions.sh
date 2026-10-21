#!/bin/sh
# NEVER run on a workstation. This mutates only a disposable Fedora container,
# the user-namespace root created by tests/sandbox/fedora-userns.sh, or a
# disposable distroproof guest (DNF4 on Rocky 8 and 9, ADR 016).
# PKG (default hello) is installed and removed; DRIFT (default tree) is the
# package installed behind a saved plan's back.
set -eu
PKG=${PKG:-hello}
DRIFT=${DRIFT:-tree}
if command -v dnf5 >/dev/null 2>&1; then DNF=dnf5; PROTECTED=dnf5; else DNF=dnf; PROTECTED=dnf; fi
installed() { rpm -q "$1" >/dev/null 2>&1; }
fail() { echo "$1" >&2; exit 1; }
! installed "$PKG" || fail "$PKG must be absent before the check"
! installed "$DRIFT" || fail "$DRIFT must be absent before the check"
cat > /tmp/hello.toml <<MANIFEST
version = 1
[packages.dnf]
install = ['$PKG']
MANIFEST
xpac apply -f /tmp/hello.toml --dry-run --json
if printf 'no\n' | xpac apply -f /tmp/hello.toml; then
    fail 'unconfirmed apply unexpectedly succeeded'
fi
! installed "$PKG" || fail 'unconfirmed apply changed the system'
xpac apply -f /tmp/hello.toml --yes --state-dir /tmp/xpac-records --json
installed "$PKG"
$DNF repoquery --userinstalled --qf '%{name}\n' 2>/dev/null | grep -qx "$PKG" || fail 'request not user-installed'
xpac apply -f /tmp/hello.toml --yes --json > /tmp/second-apply.json
grep -q '"operations":\[\]' /tmp/second-apply.json
if xpac remove "dnf:$PROTECTED" --dry-run; then
    fail 'protected-package removal preview unexpectedly succeeded'
fi
xpac plan "dnf:$PKG" --remove --save /tmp/stale.json --json
$DNF install -y "$DRIFT" >/dev/null
if xpac apply --plan /tmp/stale.json --yes --json; then
    fail 'stale saved plan unexpectedly executed'
fi
installed "$PKG"
xpac plan "dnf:$PKG" --remove --save /tmp/review.json --expires-in 300 --json
xpac apply --plan /tmp/review.json --yes --state-dir /tmp/xpac-records --json
! installed "$PKG" || fail 'reviewed removal left the package installed'
xpac history --state-dir /tmp/xpac-records --json > /tmp/history.json
[ "$(grep -o '"success":true' /tmp/history.json | wc -l)" -ge 2 ] || fail 'journal is missing results'
