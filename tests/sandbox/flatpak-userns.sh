#!/bin/sh
# Disposable Arch root with Flatpak for native Flatpak validation, built rootlessly.
#
# Usage: tests/sandbox/flatpak-userns.sh WORKDIR STATIC_XPAC
#
# The root is created by host pacman with every path redirected into WORKDIR
# (Arch-signed packages) inside a user namespace mapping the subordinate UID
# range, then checks run chrooted into it. The checks build their own tiny
# runtime and app in a local repository; nothing is fetched from Flatpak remotes.
# CHECK_DIR (default tests/containers) supplies the check scripts.
set -eu
WORK=${1:?usage: flatpak-userns.sh WORKDIR STATIC_XPAC}
XPAC=${2:?usage: flatpak-userns.sh WORKDIR STATIC_XPAC}
CHECKS=${CHECKS:-flatpak-transactions.sh}
HERE=$(cd "$(dirname "$0")" && pwd)
CHECK_DIR=$(cd "${CHECK_DIR:-$HERE/../containers}" && pwd)
[ "$(id -u)" -ne 0 ] || { echo 'refusing to run as real root' >&2; exit 1; }
full() {
    unshare --map-auto --map-root-user --mount --pid --fork --kill-child "$@"
}
mkdir -p "$WORK/cache"
WORK=$(cd "$WORK" && pwd)
ROOT="$WORK/flatpak-root"
if [ ! -e "$ROOT/etc/pacman.conf" ]; then
    mkdir -p "$ROOT/var/lib/pacman" "$ROOT/etc/pacman.d/gnupg" "$ROOT/etc/pacman.d/hooks"
    cp /etc/pacman.d/mirrorlist "$ROOT/etc/pacman.d/mirrorlist"
    for file in pubring.gpg trustdb.gpg gpg.conf; do
        cp "/etc/pacman.d/gnupg/$file" "$ROOT/etc/pacman.d/gnupg/$file"
    done
    cat > "$ROOT/etc/pacman.conf" <<'CONF'
[options]
Architecture = auto
SigLevel = Required DatabaseOptional
DisableSandbox
[core]
Include = /etc/pacman.d/mirrorlist
[extra]
Include = /etc/pacman.d/mirrorlist
CONF
fi
full env LC_ALL=C pacman --root "$ROOT" --dbpath "$ROOT/var/lib/pacman" \
    --cachedir "$WORK/cache" --gpgdir "$ROOT/etc/pacman.d/gnupg" \
    --config "$ROOT/etc/pacman.conf" --hookdir "$ROOT/etc/pacman.d/hooks" \
    --sync --refresh --needed --noconfirm --noprogressbar \
    flatpak coreutils grep sed
export ROOT CHECKS XPAC CHECK_DIR
full sh -eu <<'INNER'
mkdir -p "$ROOT/checks" "$ROOT/usr/local/bin" "$ROOT/tmp" "$ROOT/root"
mount -t tmpfs -o mode=1777 tmpfs "$ROOT/tmp"
mount -t tmpfs -o mode=0700 tmpfs "$ROOT/root"
cp "$XPAC" "$ROOT/usr/local/bin/xpac"
mount --bind "$CHECK_DIR" "$ROOT/checks"
mount --rbind /dev "$ROOT/dev"
mount -t proc proc "$ROOT/proc"
for check in $CHECKS; do
    echo "== $check"
    chroot "$ROOT" /usr/bin/env -i PATH=/usr/local/bin:/usr/bin HOME=/root \
        LC_ALL=C sh "/checks/$check"
done
INNER
echo 'per-run state (/tmp, /root) removed with the namespace'
