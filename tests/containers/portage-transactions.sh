#!/bin/sh
# NEVER run on a workstation. This mutates only a disposable Gentoo guest.
set -eu
installed() { [ -n "$(portageq match / "$1")" ]; }
in_world() { grep -q "^$1\(::.*\)\?$" /var/lib/portage/world; }
fail() { echo "$1" >&2; exit 1; }
! installed app-text/tree || fail 'tree must be absent before the check'
cat > /tmp/tree.toml <<'MANIFEST'
version = 1
[packages.portage]
install = ['app-text/tree']
MANIFEST
xpac apply -f /tmp/tree.toml --dry-run --json
if printf 'no\n' | xpac apply -f /tmp/tree.toml; then
    fail 'unconfirmed apply unexpectedly succeeded'
fi
! installed app-text/tree || fail 'unconfirmed apply changed the system'
xpac apply -f /tmp/tree.toml --yes --state-dir /tmp/xpac-records --json
installed app-text/tree && in_world app-text/tree || fail 'requested package is not installed in world'
xpac apply -f /tmp/tree.toml --yes --json > /tmp/second-apply.json
grep -q '"operations":\[\]' /tmp/second-apply.json
# What installed packages need is never removed.
if xpac remove portage:sys-libs/glibc --dry-run; then
    fail 'removal of a needed package unexpectedly previewed'
fi
xpac plan portage:app-text/tree --remove --save /tmp/stale.json --json
xpac install portage:app-misc/figlet --yes --state-dir /tmp/xpac-records --json
installed app-misc/figlet || fail 'figlet was not installed'
if xpac apply --plan /tmp/stale.json --yes --json; then
    fail 'stale saved plan unexpectedly executed'
fi
installed app-text/tree || fail 'stale plan changed the system'
xpac plan portage:app-text/tree --remove --save /tmp/review.json --expires-in 300 --json
xpac apply --plan /tmp/review.json --yes --state-dir /tmp/xpac-records --json
! installed app-text/tree && ! in_world app-text/tree || fail 'reviewed removal left the package or its world entry'
# A dependency asked for is recorded in world, not rebuilt.
dep=sys-libs/ncurses
! in_world "$dep" || fail "$dep is already selected"
before=$(portageq match / "$dep")
xpac install "portage:$dep" --yes --state-dir /tmp/xpac-records --json
in_world "$dep" && [ "$(portageq match / "$dep")" = "$before" ] || fail "$dep was not selected in place"
xpac remove portage:app-misc/figlet --yes --state-dir /tmp/xpac-records --json
! installed app-misc/figlet || fail 'figlet removal left the package'
xpac history --state-dir /tmp/xpac-records --json > /tmp/history.json
[ "$(grep -o '"success":true' /tmp/history.json | wc -l)" -ge 5 ] || fail 'journal is missing results'
xpac doctor --state-dir /tmp/xpac-records --json
