#!/bin/sh
# Disposable Debian/Ubuntu-style root for native APT validation, built rootlessly.
#
# Usage: tests/sandbox/debian-userns.sh WORKDIR STATIC_XPAC [SUITE [MIRROR]] [-- CHECK...]
#
# A disposable Arch tool root provides debootstrap and the Debian archive keyring
# (Arch-signed packages). Everything runs in a user namespace mapping the
# subordinate UID range (unshare --map-auto), so dpkg can own files as Debian
# expects. The host package databases are never targets: host files owned by the
# real root are not writable from the namespace. STATIC_XPAC must be static (musl).
# The Debian root lives on a tmpfs mounted inside the namespace: setuid sudo works
# there (the scratch /tmp is nosuid) and the root disappears with the namespace.
set -eu
WORK=${1:?usage: debian-userns.sh WORKDIR STATIC_XPAC [SUITE [MIRROR]]}
XPAC=${2:?usage: debian-userns.sh WORKDIR STATIC_XPAC [SUITE [MIRROR]]}
SUITE=${3:-bookworm}
MIRROR=${4:-http://deb.debian.org/debian}
CHECKS=${CHECKS:-inspection.sh transactions.sh privilege.sh privilege-pam.sh}
HERE=$(cd "$(dirname "$0")" && pwd)
CHECK_DIR=$(cd "${CHECK_DIR:-$HERE/../containers}" && pwd)
[ "$(id -u)" -ne 0 ] || { echo 'refusing to run as real root' >&2; exit 1; }
full() {
    unshare --map-auto --map-root-user --mount --pid --fork --kill-child "$@"
}
mkdir -p "$WORK/cache"
WORK=$(cd "$WORK" && pwd)
TOOLS="$WORK/tools"
if [ ! -e "$TOOLS/etc/pacman.conf" ]; then
    mkdir -p "$TOOLS/var/lib/pacman" "$TOOLS/etc/pacman.d/gnupg" \
        "$TOOLS/etc/pacman.d/hooks" "$TOOLS/srv"
    cp /etc/pacman.d/mirrorlist "$TOOLS/etc/pacman.d/mirrorlist"
    for file in pubring.gpg trustdb.gpg gpg.conf; do
        cp "/etc/pacman.d/gnupg/$file" "$TOOLS/etc/pacman.d/gnupg/$file"
    done
    cat > "$TOOLS/etc/pacman.conf" <<'CONF'
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
full env LC_ALL=C pacman --root "$TOOLS" --dbpath "$TOOLS/var/lib/pacman" \
    --cachedir "$WORK/cache" --gpgdir "$TOOLS/etc/pacman.d/gnupg" \
    --config "$TOOLS/etc/pacman.conf" --hookdir "$TOOLS/etc/pacman.d/hooks" \
    --sync --refresh --needed --noconfirm --noprogressbar \
    debootstrap debian-archive-keyring ubuntu-keyring ca-certificates \
    coreutils grep sed gawk tar gzip xz zstd binutils findutils util-linux
NAME=$(basename "$(mktemp -d "$TOOLS/srv/$SUITE.XXXXXX")")
export TOOLS NAME SUITE MIRROR CHECKS XPAC CHECK_DIR
full sh -eu <<'INNER'
mount -t tmpfs -o mode=0755 tmpfs "$TOOLS/srv/$NAME"
mkdir -p "$TOOLS/var/cache/debootstrap"
mount --rbind /dev "$TOOLS/dev"
mount -t proc proc "$TOOLS/proc"
cp /etc/resolv.conf "$TOOLS/etc/resolv.conf"
keyring=/usr/share/keyrings/debian-archive-keyring.gpg
case "$MIRROR" in *ubuntu*) keyring=/usr/share/keyrings/ubuntu-archive-keyring.gpg ;; esac
chroot "$TOOLS" /usr/bin/env -i PATH=/usr/bin LC_ALL=C \
    debootstrap --variant=minbase --arch=amd64 --keyring="$keyring" \
    --cache-dir=/var/cache/debootstrap "$SUITE" "/srv/$NAME" "$MIRROR"
DEB="$TOOLS/srv/$NAME"
mkdir -p "$DEB/checks"
cp "$XPAC" "$DEB/usr/local/bin/xpac"
mount --bind "$CHECK_DIR" "$DEB/checks"
mount --rbind /dev "$DEB/dev"
# A private devpts instance lets sudo's use_pty allocate terminals in the namespace.
mount -t devpts -o newinstance,ptmxmode=0666,mode=0620 devpts "$DEB/dev/pts"
mount --bind "$DEB/dev/pts/ptmx" "$DEB/dev/ptmx"
mount -t proc proc "$DEB/proc"
cp /etc/resolv.conf "$DEB/etc/resolv.conf"
in_deb() {
    chroot "$DEB" /usr/bin/env -i \
        PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
        HOME=/root LC_ALL=C DEBIAN_FRONTEND=noninteractive "$@"
}
in_deb apt-get update -qq
for check in $CHECKS; do
    echo "== $check"
    in_deb sh "/checks/$check"
done
INNER
rmdir "$TOOLS/srv/$NAME"
echo 'disposable root removed with its namespace'
