#!/bin/sh
# NEVER run on a workstation: disposable root only (tests/sandbox/brew-userns.sh),
# as an unprivileged user. Installs Homebrew from its official repository first.
set -eu
fail() { echo "$1" >&2; exit 1; }
prefix=/home/linuxbrew/.linuxbrew
if [ ! -x "$prefix/bin/brew" ]; then
    tag=$(git ls-remote --tags --refs https://github.com/Homebrew/brew | sed 's|.*refs/tags/||' | sort -V | tail -1)
    git clone -q --depth 1 --branch "$tag" https://github.com/Homebrew/brew "$prefix/Homebrew"
    mkdir -p "$prefix/bin" && ln -s ../Homebrew/bin/brew "$prefix/bin/brew"
fi
export HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_ANALYTICS=1
installed() { brew list --formula -1 2>/dev/null | grep -qx "$1"; }
brew --version
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
[ "$(grep -o '"success":true' /tmp/history.json | wc -l)" -ge 3 ] || fail 'journal is missing results'
echo 'homebrew checks passed'
