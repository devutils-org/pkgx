#!/bin/sh
# NEVER run on a workstation. This mutates only the disposable CI container.
set -eu
cat > /tmp/hello.toml <<'MANIFEST'
version = 1
[packages.apt]
install = ['hello']
MANIFEST
xpac apply -f /tmp/hello.toml --dry-run --json
xpac apply -f /tmp/hello.toml --yes --json
[ "$(dpkg-query -W -f='${db:Status-Status}' hello)" = installed ]
xpac apply -f /tmp/hello.toml --yes --json > /tmp/second-apply.json
# No operations should be needed on repeat application.
grep -q '"operations":\[\]' /tmp/second-apply.json
apt-mark hold hello
if xpac remove apt:hello --dry-run; then
    echo 'held-package removal unexpectedly succeeded' >&2
    exit 1
fi
apt-mark unhold hello
if printf 'no\n' | xpac remove apt:hello; then
    echo 'noninteractive unconfirmed removal unexpectedly succeeded' >&2
    exit 1
fi
[ "$(dpkg-query -W -f='${db:Status-Status}' hello)" = installed ]
if xpac remove apt:dpkg --dry-run; then
    echo 'essential-package removal unexpectedly succeeded' >&2
    exit 1
fi
xpac remove apt:hello --yes --json
status=$(dpkg-query -W -f='${db:Status-Status}' hello 2>/dev/null || true)
[ "$status" != installed ]
# Save, review and execute a native manual-reason transition on an installed tool.
xpac install apt:hello --yes --json
apt-mark auto hello
xpac plan apt:hello --save /tmp/hello-review.json --expires-in 300 --json
xpac apply --plan /tmp/hello-review.json --yes --state-dir /tmp/xpac-records --json
[ "$(apt-mark showmanual hello)" = hello ]
# Changing the baseline after review must invalidate the saved removal plan.
xpac plan apt:hello --remove --save /tmp/stale-review.json --json
apt-mark auto hello
if xpac apply --plan /tmp/stale-review.json --yes --json; then
    echo 'stale saved plan unexpectedly executed' >&2
    exit 1
fi
[ "$(dpkg-query -W -f='${db:Status-Status}' hello)" = installed ]
# Strict mode affects only a separately reviewed grant, not every installed tool.
arch=$(dpkg --print-architecture)
xpac manage "apt:hello:$arch" --state-dir /tmp/xpac-strict --dry-run --json
[ ! -e /tmp/xpac-strict ]
xpac manage "apt:hello:$arch" --state-dir /tmp/xpac-strict --yes --json
cat > /tmp/strict.toml <<'MANIFEST'
version = 1
[system]
mode = 'strict'
[packages.apt]
install = []
MANIFEST
xpac diff -f /tmp/strict.toml --state-dir /tmp/xpac-strict --json
xpac apply -f /tmp/strict.toml --state-dir /tmp/xpac-strict --dry-run --json
xpac apply -f /tmp/strict.toml --state-dir /tmp/xpac-strict --yes --json
status=$(dpkg-query -W -f='${db:Status-Status}' hello 2>/dev/null || true)
[ "$status" != installed ]
[ "$(dpkg-query -W -f='${db:Status-Status}' bash)" = installed ]
xpac apply -f /tmp/strict.toml --state-dir /tmp/xpac-strict --yes --json > /tmp/strict-repeat.json
grep -q '"operations":\[\]' /tmp/strict-repeat.json
xpac manage "apt:hello:$arch" --forget --yes --state-dir /tmp/xpac-strict --json
