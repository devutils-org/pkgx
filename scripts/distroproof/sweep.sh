#!/bin/sh
set -eu
[ -n "${DP_TARGET:-}" ] && [ "$PWD" = /distroproof/work ] &&
    [ "$(cat /etc/xpac-distroproof-guest)" = "$DP_TARGET" ] || {
    echo 'sweep requires the prepared disposable distroproof guest' >&2; exit 1;
}
# The distroproof root command has no login environment. Give it a guest HOME.
if [ "$(id -u)" = 0 ]; then HOME=/root; export HOME; fi
exec python3 scripts/distroproof/sweep.py
