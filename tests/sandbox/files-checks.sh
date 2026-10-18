#!/bin/sh
# Runs inside tests/sandbox/files-userns.sh (read-only host, private /tmp), or on a
# disposable CI runner with PKGX_DISPOSABLE=1, PKGX_BIN and PKGX_ROOT set.
set -eu
fail() { echo "FAIL: $1" >&2; exit 1; }
pass() { echo "ok: $1"; }
ROOT=${PKGX_ROOT:-/tmp}
BIN=${PKGX_BIN:-/tmp/bin/pkgx}
if [ "$ROOT" != /tmp ] && [ "${PKGX_DISPOSABLE:-}" != 1 ]; then
    fail 'outside the sandbox, run only on a disposable runner (PKGX_DISPOSABLE=1)'
fi
export HOME="$ROOT/home"
mkdir -p "$HOME" "$ROOT/repo/dotfiles" "$ROOT/outside"
cd "$ROOT/repo"
pkgx() { "$BIN" --state-dir "$ROOT/state" "$@" </dev/null; }
mode() { stat -c %a "$1" 2>/dev/null || stat -f %Lp "$1"; }
retarget() { sed "s#$1#$2#" system.toml > system.toml.new && mv system.toml.new system.toml; }

printf 'set -g mouse on\n' > dotfiles/tmux.conf
printf 'alias ll="ls -l"\n' > dotfiles/bashrc
printf 'x = 1\n' > dotfiles/evil
printf 'existing\n' > "$HOME/.bashrc"
cat > system.toml <<'TOML'
version = 1
[files."~/.config/tmux/tmux.conf"]
source = "dotfiles/tmux.conf"
mode = "0644"
[files."~/.bashrc"]
source = "dotfiles/bashrc"
TOML

pkgx diff -f system.toml | grep -q 'file unmanaged conflict	~/.bashrc' || fail 'conflict not reported'
pkgx diff -f system.toml | grep -q 'file missing	~/.config/tmux/tmux.conf' || fail 'missing not reported'
if pkgx apply -f system.toml --yes 2>$ROOT/err; then fail 'apply over unmanaged file'; fi
grep -q 'unmanaged_conflict' $ROOT/err || fail 'conflict refusal not named'
[ ! -e "$HOME/.config" ] && [ ! -e $ROOT/state/files ] || fail 'refused apply changed state'
pass 'unmanaged files block apply without side effects'

if pkgx file adopt "$HOME/.bashrc" 2>/dev/null; then fail 'adoption without --yes'; fi
pkgx file adopt "$HOME/.bashrc" --yes >/dev/null
backup=$(ls $ROOT/state/files/backups)
[ "$(cat "$ROOT/state/files/backups/$backup")" = existing ] || fail 'backup content'
[ "$(mode "$ROOT/state/files/backups/$backup")" = 600 ] || fail 'backup mode'
[ "$(mode $ROOT/state/files)" = 700 ] || fail 'registry directory mode'
pass 'adoption backs up privately'

pkgx plan -f system.toml | grep -q '^+alias ll' || fail 'plan diff'
pkgx apply -f system.toml --yes | grep -q 'create ~/.config/tmux/tmux.conf — verified' || fail 'apply result'
[ "$(cat "$HOME/.bashrc")" = 'alias ll="ls -l"' ] || fail 'adopted file not replaced'
[ "$(mode "$HOME/.bashrc")" = 600 ] || fail 'default mode'
[ "$(mode "$HOME/.config/tmux/tmux.conf")" = 644 ] || fail 'declared mode'
[ "$(mode "$HOME/.config")" = 700 ] && [ "$(mode "$HOME/.config/tmux")" = 700 ] || fail 'parent modes'
pkgx apply -f system.toml --yes | grep -q 'No file writes' || fail 'repeated apply not a no-op'
ls -A "$HOME" "$HOME/.config/tmux" | grep -q '\.pkgx-' && fail 'temporary file left'
pass 'apply writes atomically and is idempotent'

echo '# local' >> "$HOME/.config/tmux/tmux.conf"
if pkgx apply -f system.toml --yes 2>$ROOT/err; then fail 'drift overwritten'; fi
grep -q 'drifted' $ROOT/err || fail 'drift refusal not named'
grep -q '# local' "$HOME/.config/tmux/tmux.conf" || fail 'local edit lost'
printf 'set -g mouse on\n# local\n' > dotfiles/tmux.conf
pkgx diff -f system.toml | grep -q 'file in sync	~/.config/tmux/tmux.conf' || fail 'edit copied back not in sync'
pass 'local edits are drift, never reverted'

printf 'evil = 1\n' > $ROOT/outside/victim
cat >> system.toml <<'TOML'
[files."~/.evil"]
source = "dotfiles/evil"
[files."~/linked/conf"]
source = "dotfiles/evil"
TOML
ln -s $ROOT/outside/victim "$HOME/.evil"
ln -s $ROOT/outside "$HOME/linked"
pkgx diff -f system.toml | grep -q 'file unsupported	~/.evil' || fail 'symlink target'
pkgx diff -f system.toml | grep -q 'file unsupported	~/linked/conf' || fail 'symlinked ancestor'
if pkgx apply -f system.toml --yes 2>/dev/null; then fail 'apply through symlink'; fi
[ "$(cat $ROOT/outside/victim)" = 'evil = 1' ] && [ ! -e $ROOT/outside/conf ] || fail 'symlink followed'
rm "$HOME/.evil" "$HOME/linked"
ln -s $ROOT/outside/victim dotfiles/evil.link
retarget '"dotfiles/evil"' '"dotfiles/evil.link"'
if pkgx diff -f system.toml 2>/dev/null; then fail 'source symlink escape'; fi
retarget '"dotfiles/evil.link"' '"dotfiles/evil"'
pass 'symlinked targets, ancestors and sources are refused'

: > $ROOT/state/files/lock
chmod 600 $ROOT/state/files/lock
if pkgx apply -f system.toml --yes 2>$ROOT/err; then fail 'apply while locked'; fi
grep -q 'locked' $ROOT/err || fail 'lock refusal not named'
[ ! -e "$HOME/.evil" ] || fail 'locked apply wrote'
rm $ROOT/state/files/lock
pkgx apply -f system.toml --yes >/dev/null
[ "$(cat "$HOME/linked/conf")" = 'x = 1' ] || fail 'apply after unlock'
pass 'a held lock refuses writes'

echo edit >> "$HOME/.evil"
if pkgx file remove '~/.evil' --yes 2>/dev/null; then fail 'removed drifted file'; fi
[ -e "$HOME/.evil" ] || fail 'drifted file deleted'
pkgx file remove '~/.bashrc' --yes >/dev/null
[ ! -e "$HOME/.bashrc" ] || fail 'managed file not removed'
pkgx file list | grep -q '"~/.bashrc"' && fail 'receipt kept after removal'
pkgx file forget '~/.evil' --yes >/dev/null
[ -e "$HOME/.evil" ] || fail 'forget touched file'
pass 'removal deletes only unchanged managed files; forget keeps files'
echo 'all managed-file sandbox checks passed'
