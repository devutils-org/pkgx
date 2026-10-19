#!/bin/sh
# Disposable Arch root with Homebrew on Linux for native Homebrew validation.
#
# Usage: tests/sandbox/brew-userns.sh WORKDIR STATIC_XPAC
#
# The root is created by host pacman with every path redirected into WORKDIR
# (Arch-signed packages) inside a user namespace mapping the subordinate UID
# range, then checks run chrooted into it as the unprivileged `brewer` user, since
# Homebrew refuses root. Homebrew itself (git) and bottles are downloaded from the
# official Homebrew sources into the disposable root only.
# CHECK_DIR (default tests/containers) supplies the check scripts.
set -eu
WORK=${1:?usage: brew-userns.sh WORKDIR STATIC_XPAC}
XPAC=${2:?usage: brew-userns.sh WORKDIR STATIC_XPAC}
CHECKS=${CHECKS:-brew-transactions.sh}
HERE=$(cd "$(dirname "$0")" && pwd)
CHECK_DIR=$(cd "${CHECK_DIR:-$HERE/../containers}" && pwd)
[ "$(id -u)" -ne 0 ] || { echo 'refusing to run as real root' >&2; exit 1; }
full() {
    unshare --map-auto --map-root-user --mount --pid --fork --kill-child "$@"
}
mkdir -p "$WORK/cache"
WORK=$(cd "$WORK" && pwd)
ROOT="$WORK/brew-root"
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
    git curl ca-certificates coreutils grep sed util-linux shadow which procps-ng file gcc-libs glibc tar gzip ruby
export ROOT CHECKS XPAC CHECK_DIR
full sh -eu <<'INNER'
mkdir -p "$ROOT/checks" "$ROOT/usr/local/bin" "$ROOT/tmp" "$ROOT/home"
mount -t tmpfs -o mode=1777 tmpfs "$ROOT/tmp"
mount -t tmpfs -o mode=0755 tmpfs "$ROOT/home"
cp "$XPAC" "$ROOT/usr/local/bin/xpac"
mount --bind "$CHECK_DIR" "$ROOT/checks"
mount --rbind /dev "$ROOT/dev"
mount -t proc proc "$ROOT/proc"
cp /etc/resolv.conf "$ROOT/etc/resolv.conf"
chroot "$ROOT" sh -c 'id brewer >/dev/null 2>&1 || useradd -m -u 1000 brewer;
    mkdir -p /home/brewer /home/linuxbrew && chown brewer: /home/brewer /home/linuxbrew'
for check in $CHECKS; do
    echo "== $check"
    chroot "$ROOT" /usr/bin/setpriv --reuid=1000 --regid=1000 --init-groups \
        /usr/bin/env -i PATH=/home/linuxbrew/.linuxbrew/bin:/usr/local/bin:/usr/bin \
        HOME=/home/brewer LC_ALL=C.UTF-8 XPAC_DISPOSABLE=1 sh "/checks/$check"
done
INNER
echo 'per-run state (/tmp, /home) removed with the namespace'
