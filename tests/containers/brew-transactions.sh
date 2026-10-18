#!/bin/sh
# NEVER run on a workstation: disposable environments only, as an unprivileged
# user. On Linux, tests/sandbox/brew-userns.sh provides the root and Homebrew is
# installed from its official repository first. On macOS, use a disposable VM or
# CI runner with Homebrew preinstalled: preinstalled jq/oniguruma are uninstalled
# and `clean --orphans` removes every orphaned formula of that environment.
set -eu
fail() { echo "$1" >&2; exit 1; }
[ "${PKGX_DISPOSABLE:-}" = 1 ] || fail 'refusing to run: set PKGX_DISPOSABLE=1 in a disposable environment only'
prefix=/home/linuxbrew/.linuxbrew
if ! command -v brew >/dev/null && [ "$(uname -s)" = Linux ] && [ ! -x "$prefix/bin/brew" ]; then
    tag=$(git ls-remote --tags --refs https://github.com/Homebrew/brew | sed 's|.*refs/tags/||' | sort -V | tail -1)
    git clone -q --depth 1 --branch "$tag" https://github.com/Homebrew/brew "$prefix/Homebrew"
    mkdir -p "$prefix/bin" && ln -s ../Homebrew/bin/brew "$prefix/bin/brew"
fi
command -v brew >/dev/null || fail 'brew not found on PATH'
export HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_ANALYTICS=1
installed() { brew list --formula -1 2>/dev/null | grep -qx "$1"; }
brew --version
# Start from a known state: the scenario needs jq and its dependency absent.
for formula in jq oniguruma; do
    ! installed "$formula" || brew uninstall --formula --ignore-dependencies "$formula"
done
pkgx list --provider brew --json
pkgx install brew:jq:formula --dry-run --json
if printf 'no\n' | pkgx install brew:jq:formula; then
    fail 'unconfirmed install unexpectedly ran'
fi
! installed jq || fail 'unconfirmed install changed the system'
cat > /tmp/brew.toml <<'MANIFEST'
version = 1
[packages.brew]
install = ['jq:formula']
MANIFEST
pkgx apply -f /tmp/brew.toml --yes --state-dir /tmp/pkgx-records --json
installed jq && installed oniguruma || fail 'jq or its dependency missing'
[ "$(jq -n 1)" = 1 ] || fail 'installed jq does not run'
pkgx apply -f /tmp/brew.toml --yes --json | grep -q '"operations":\[\]'
pkgx why brew:oniguruma:formula --json | grep -q '"install_reason":"dependency"'
if pkgx remove brew:oniguruma:formula --dry-run; then
    fail 'removal of a required formula unexpectedly previewed'
fi
pkgx remove brew:jq:formula --yes --state-dir /tmp/pkgx-records --json
! installed jq || fail 'jq still installed'
installed oniguruma || fail 'removal cascaded into a dependency'
pkgx audit --provider brew --json | grep -q oniguruma
pkgx clean --orphans --provider brew --yes --state-dir /tmp/pkgx-records --json
! installed oniguruma || fail 'orphan cleanup left oniguruma'
pkgx history --state-dir /tmp/pkgx-records --json > /tmp/history.json
[ "$(grep -o '"success":true' /tmp/history.json | wc -l | tr -d ' ')" -ge 3 ] || fail 'journal is missing results'
echo 'homebrew checks passed'
