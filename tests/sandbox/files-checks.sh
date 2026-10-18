#!/bin/sh
# Runs only inside tests/sandbox/files-userns.sh: read-only host, private /tmp.
set -eu
fail() { echo "FAIL: $1" >&2; exit 1; }
pass() { echo "ok: $1"; }
export HOME=/tmp/home
mkdir -p "$HOME" /tmp/repo/dotfiles /tmp/outside
cd /tmp/repo
pkgx() { /tmp/bin/pkgx --state-dir /tmp/state "$@" </dev/null; }
mode() { stat -c %a "$1"; }

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
if pkgx apply -f system.toml --yes 2>/tmp/err; then fail 'apply over unmanaged file'; fi
grep -q 'unmanaged_conflict' /tmp/err || fail 'conflict refusal not named'
[ ! -e "$HOME/.config" ] && [ ! -e /tmp/state/files ] || fail 'refused apply changed state'
pass 'unmanaged files block apply without side effects'

if pkgx file adopt "$HOME/.bashrc" 2>/dev/null; then fail 'adoption without --yes'; fi
pkgx file adopt "$HOME/.bashrc" --yes >/dev/null
backup=$(ls /tmp/state/files/backups)
[ "$(cat "/tmp/state/files/backups/$backup")" = existing ] || fail 'backup content'
[ "$(mode "/tmp/state/files/backups/$backup")" = 600 ] || fail 'backup mode'
[ "$(mode /tmp/state/files)" = 700 ] || fail 'registry directory mode'
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
if pkgx apply -f system.toml --yes 2>/tmp/err; then fail 'drift overwritten'; fi
grep -q 'drifted' /tmp/err || fail 'drift refusal not named'
grep -q '# local' "$HOME/.config/tmux/tmux.conf" || fail 'local edit lost'
printf 'set -g mouse on\n# local\n' > dotfiles/tmux.conf
pkgx diff -f system.toml | grep -q 'file in sync	~/.config/tmux/tmux.conf' || fail 'edit copied back not in sync'
pass 'local edits are drift, never reverted'

printf 'evil = 1\n' > /tmp/outside/victim
cat >> system.toml <<'TOML'
[files."~/.evil"]
source = "dotfiles/evil"
[files."~/linked/conf"]
source = "dotfiles/evil"
TOML
ln -s /tmp/outside/victim "$HOME/.evil"
ln -s /tmp/outside "$HOME/linked"
pkgx diff -f system.toml | grep -q 'file unsupported	~/.evil' || fail 'symlink target'
pkgx diff -f system.toml | grep -q 'file unsupported	~/linked/conf' || fail 'symlinked ancestor'
if pkgx apply -f system.toml --yes 2>/dev/null; then fail 'apply through symlink'; fi
[ "$(cat /tmp/outside/victim)" = 'evil = 1' ] && [ ! -e /tmp/outside/conf ] || fail 'symlink followed'
rm "$HOME/.evil" "$HOME/linked"
ln -s /tmp/outside/victim dotfiles/evil.link
sed -i 's#"dotfiles/evil"#"dotfiles/evil.link"#' system.toml
if pkgx diff -f system.toml 2>/dev/null; then fail 'source symlink escape'; fi
sed -i 's#"dotfiles/evil.link"#"dotfiles/evil"#' system.toml
pass 'symlinked targets, ancestors and sources are refused'

: > /tmp/state/files/lock
chmod 600 /tmp/state/files/lock
if pkgx apply -f system.toml --yes 2>/tmp/err; then fail 'apply while locked'; fi
grep -q 'locked' /tmp/err || fail 'lock refusal not named'
[ ! -e "$HOME/.evil" ] || fail 'locked apply wrote'
rm /tmp/state/files/lock
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
