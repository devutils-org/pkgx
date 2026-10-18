#!/bin/sh
# NEVER run on a workstation. This mutates only a disposable Fedora container or
# the user-namespace root created by tests/sandbox/fedora-userns.sh.
set -eu
installed() { rpm -q "$1" >/dev/null 2>&1; }
fail() { echo "$1" >&2; exit 1; }
! installed hello || fail 'hello must be absent before the check'
cat > /tmp/hello.toml <<'MANIFEST'
version = 1
[packages.dnf]
install = ['hello']
MANIFEST
pkgx apply -f /tmp/hello.toml --dry-run --json
if printf 'no\n' | pkgx apply -f /tmp/hello.toml; then
    fail 'unconfirmed apply unexpectedly succeeded'
fi
! installed hello || fail 'unconfirmed apply changed the system'
pkgx apply -f /tmp/hello.toml --yes --state-dir /tmp/pkgx-records --json
installed hello
dnf5 repoquery --userinstalled --qf '%{name}\n' | grep -qx hello || fail 'request not user-installed'
pkgx apply -f /tmp/hello.toml --yes --json > /tmp/second-apply.json
grep -q '"operations":\[\]' /tmp/second-apply.json
if pkgx remove dnf:dnf5 --dry-run; then
    fail 'protected-package removal preview unexpectedly succeeded'
fi
pkgx plan dnf:hello --remove --save /tmp/stale.json --json
dnf5 install -y tree >/dev/null
if pkgx apply --plan /tmp/stale.json --yes --json; then
    fail 'stale saved plan unexpectedly executed'
fi
installed hello
pkgx plan dnf:hello --remove --save /tmp/review.json --expires-in 300 --json
pkgx apply --plan /tmp/review.json --yes --state-dir /tmp/pkgx-records --json
! installed hello || fail 'reviewed removal left the package installed'
pkgx history --state-dir /tmp/pkgx-records --json > /tmp/history.json
[ "$(grep -o '"success":true' /tmp/history.json | wc -l)" -ge 2 ] || fail 'journal is missing results'
