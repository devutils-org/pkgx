#!/bin/sh
# NEVER run on a workstation. This mutates only a disposable Arch container or
# the user-namespace root created by tests/sandbox/arch-userns.sh.
set -eu
installed() { pacman -Q "$1" >/dev/null 2>&1; }
fail() { echo "$1" >&2; exit 1; }
! installed sl || fail 'sl must be absent before the check'
cat > /tmp/sl.toml <<'MANIFEST'
version = 1
[packages.pacman]
install = ['sl']
MANIFEST
xpac apply -f /tmp/sl.toml --dry-run --json
if printf 'no\n' | xpac apply -f /tmp/sl.toml; then
    fail 'unconfirmed apply unexpectedly succeeded'
fi
! installed sl || fail 'unconfirmed apply changed the system'
xpac apply -f /tmp/sl.toml --yes --state-dir /tmp/xpac-records --json
installed sl
[ "$(pacman -Qqe sl)" = sl ] || fail 'requested package is not marked explicit'
xpac apply -f /tmp/sl.toml --yes --json > /tmp/second-apply.json
# Repeated application is a verified no-op.
grep -q '"operations":\[\]' /tmp/second-apply.json
# Native protections: HoldPkg, the built-in critical set and IgnorePkg.
cp /etc/pacman.conf /tmp/pacman.conf.orig
sed -i 's/^HoldPkg.*/& sl/' /etc/pacman.conf
if xpac remove pacman:sl --dry-run; then
    fail 'held-package removal preview unexpectedly succeeded'
fi
cp /tmp/pacman.conf.orig /etc/pacman.conf
if xpac remove pacman:filesystem --dry-run; then
    fail 'critical-package removal preview unexpectedly succeeded'
fi
sed -i 's/^HoldPkg.*/&\nIgnorePkg = figlet/' /etc/pacman.conf
if xpac install pacman:figlet --yes; then
    fail 'ignored package installation unexpectedly succeeded'
fi
cp /tmp/pacman.conf.orig /etc/pacman.conf
! installed figlet || fail 'ignored package was installed'
# A reviewed removal plan becomes stale when the inventory changes.
xpac plan pacman:sl --remove --save /tmp/stale.json --json
xpac install pacman:figlet --yes --json
installed figlet
if xpac apply --plan /tmp/stale.json --yes --json; then
    fail 'stale saved plan unexpectedly executed'
fi
installed sl
xpac plan pacman:sl --remove --save /tmp/review.json --expires-in 300 --json
xpac apply --plan /tmp/review.json --yes --state-dir /tmp/xpac-records --json
! installed sl || fail 'reviewed removal left the package installed'
installed ncurses || fail 'removal cascaded into a dependency'
xpac remove pacman:figlet --yes --json
# Suggested cleanup: a dependency-marked leaf is removed only through review.
pacman -S --noconfirm --asdeps figlet
xpac audit --provider pacman --json | grep -q '"category":"suggested_cleanup"'
xpac clean --orphans --provider pacman --dry-run --json | grep -q figlet
if printf 'no\n' | xpac clean --orphans --provider pacman; then
    fail 'unconfirmed orphan cleanup unexpectedly ran'
fi
installed figlet
xpac clean --orphans --provider pacman --yes --state-dir /tmp/xpac-records --json
! installed figlet || fail 'orphan cleanup left the package'
installed bash
# Safe native cleanup through pacman -Sc only.
xpac clean --cache --provider pacman --dry-run --json
xpac clean --cache --provider pacman --yes --state-dir /tmp/xpac-records --json > /tmp/cache.json
grep -q '"success":true' /tmp/cache.json
xpac history --state-dir /tmp/xpac-records --json > /tmp/history.json
[ "$(grep -o '"success":true' /tmp/history.json | wc -l)" -ge 4 ] || fail 'journal is missing results'
xpac doctor --state-dir /tmp/xpac-records --json
