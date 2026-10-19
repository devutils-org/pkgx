#!/bin/sh
# Disposable Arch root inside an unprivileged user namespace.
#
# Usage: tests/sandbox/arch-userns.sh WORKDIR XPAC_BINARY [CHECK...]
#
# The host package database is never a target: pacman is always given an
# explicit --root/--dbpath/--cachedir under WORKDIR, host files owned by the real
# root are not writable from the namespace, and checks run with WORKDIR/root
# bound as /. Only public keyring material is copied. The root is removed
# afterwards unless KEEP_ROOT=1; WORKDIR/cache keeps downloaded packages. Requires bwrap, network
# access to the configured mirrors and an Arch host pacman.
set -eu
WORK=${1:?usage: arch-userns.sh WORKDIR XPAC_BINARY [CHECK...]}
XPAC=${2:?usage: arch-userns.sh WORKDIR XPAC_BINARY [CHECK...]}
shift 2
CHECKS=$(cd "$(dirname "$0")/../containers" && pwd)
[ "$(id -u)" -ne 0 ] || { echo 'refusing to run as real root' >&2; exit 1; }
mkdir -p "$WORK"
ROOT=$(mktemp -d "$WORK/root.XXXXXX")
# Downloaded packages are reused across disposable roots; signatures are still checked.
CACHE="$WORK/cache"
mkdir -p "$CACHE"
mkdir -p "$ROOT/var/lib/pacman" "$ROOT/var/cache/pacman/pkg" \
    "$ROOT/etc/pacman.d/gnupg" "$ROOT/etc/pacman.d/hooks" "$ROOT/tmp" "$ROOT/usr/local/bin" "$ROOT/checks"
cp /etc/pacman.d/mirrorlist "$ROOT/etc/pacman.d/mirrorlist"
for file in pubring.gpg trustdb.gpg gpg.conf; do
    cp "/etc/pacman.d/gnupg/$file" "$ROOT/etc/pacman.d/gnupg/$file"
done
cat > "$ROOT/etc/pacman.conf" <<'CONF'
[options]
Architecture = auto
HoldPkg = pacman glibc
SigLevel = Required DatabaseOptional
LocalFileSigLevel = Optional
DisableSandbox
[core]
Include = /etc/pacman.d/mirrorlist
[extra]
Include = /etc/pacman.d/mirrorlist
CONF
userns() {
    bwrap --unshare-user --uid 0 --gid 0 --unshare-pid --unshare-ipc \
        --cap-add CAP_SYS_CHROOT --die-with-parent "$@"
}
# Bootstrap: host pacman, every path redirected into the disposable root.
userns --bind / / --dev /dev --proc /proc \
    env LC_ALL=C pacman --root "$ROOT" --dbpath "$ROOT/var/lib/pacman" \
    --cachedir "$CACHE" --gpgdir "$ROOT/etc/pacman.d/gnupg" \
    --config "$ROOT/etc/pacman.conf" --hookdir "$ROOT/etc/pacman.d/hooks" \
    --sync --refresh --noconfirm --noprogressbar pacman
cp /etc/resolv.conf "$ROOT/etc/resolv.conf"
in_root() {
    userns --bind "$ROOT" / --dev /dev --proc /proc --tmpfs /tmp \
        --bind "$CACHE" /var/cache/pacman/pkg --ro-bind "$XPAC" /usr/local/bin/xpac --ro-bind "$CHECKS" /checks \
        env -i PATH=/usr/local/bin:/usr/bin HOME=/tmp LC_ALL=C "$@"
}
for check in "${@:-pacman.sh pacman-transactions.sh}"; do
    for script in $check; do
        echo "== $script"
        in_root sh "/checks/$script"
    done
done
if [ "${KEEP_ROOT:-0}" = 1 ]; then
    echo "disposable root kept: $ROOT"
else
    # Packages install some read-only directories; they belong to this user.
    chmod -R u+w "$ROOT"
    rm -rf "$ROOT"
    echo 'disposable root removed'
fi
