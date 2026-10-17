#!/bin/sh
# NEVER run on a workstation. This mutates only the disposable CI container.
set -eu
cat > /tmp/hello.toml <<'MANIFEST'
version = 1
[packages.apt]
install = ['hello']
MANIFEST
pkgx apply -f /tmp/hello.toml --dry-run --json
pkgx apply -f /tmp/hello.toml --yes --json
[ "$(dpkg-query -W -f='${db:Status-Status}' hello)" = installed ]
pkgx apply -f /tmp/hello.toml --yes --json > /tmp/second-apply.json
# No operations should be needed on repeat application.
grep -q '"operations":\[\]' /tmp/second-apply.json
apt-mark hold hello
if pkgx remove apt:hello --dry-run; then
    echo 'held-package removal unexpectedly succeeded' >&2
    exit 1
fi
apt-mark unhold hello
if printf 'no\n' | pkgx remove apt:hello; then
    echo 'noninteractive unconfirmed removal unexpectedly succeeded' >&2
    exit 1
fi
[ "$(dpkg-query -W -f='${db:Status-Status}' hello)" = installed ]
if pkgx remove apt:dpkg --dry-run; then
    echo 'essential-package removal unexpectedly succeeded' >&2
    exit 1
fi
pkgx remove apt:hello --yes --json
status=$(dpkg-query -W -f='${db:Status-Status}' hello 2>/dev/null || true)
[ "$status" != installed ]
# Save, review and execute a native manual-reason transition on an installed tool.
pkgx install apt:hello --yes --json
apt-mark auto hello
pkgx plan apt:hello --save /tmp/hello-review.json --expires-in 300 --json
pkgx apply --plan /tmp/hello-review.json --yes --state-dir /tmp/pkgx-records --json
[ "$(apt-mark showmanual hello)" = hello ]
# Changing the baseline after review must invalidate the saved removal plan.
pkgx plan apt:hello --remove --save /tmp/stale-review.json --json
apt-mark auto hello
if pkgx apply --plan /tmp/stale-review.json --yes --json; then
    echo 'stale saved plan unexpectedly executed' >&2
    exit 1
fi
[ "$(dpkg-query -W -f='${db:Status-Status}' hello)" = installed ]
# Strict mode affects only a separately reviewed grant, not every installed tool.
arch=$(dpkg --print-architecture)
pkgx manage "apt:hello:$arch" --state-dir /tmp/pkgx-strict --dry-run --json
[ ! -e /tmp/pkgx-strict ]
pkgx manage "apt:hello:$arch" --state-dir /tmp/pkgx-strict --yes --json
cat > /tmp/strict.toml <<'MANIFEST'
version = 1
[system]
mode = 'strict'
[packages.apt]
install = []
MANIFEST
pkgx diff -f /tmp/strict.toml --state-dir /tmp/pkgx-strict --json
pkgx apply -f /tmp/strict.toml --state-dir /tmp/pkgx-strict --dry-run --json
pkgx apply -f /tmp/strict.toml --state-dir /tmp/pkgx-strict --yes --json
status=$(dpkg-query -W -f='${db:Status-Status}' hello 2>/dev/null || true)
[ "$status" != installed ]
[ "$(dpkg-query -W -f='${db:Status-Status}' bash)" = installed ]
pkgx apply -f /tmp/strict.toml --state-dir /tmp/pkgx-strict --yes --json > /tmp/strict-repeat.json
grep -q '"operations":\[\]' /tmp/strict-repeat.json
pkgx manage "apt:hello:$arch" --forget --yes --state-dir /tmp/pkgx-strict --json
