#!/bin/sh
# Disposable Fedora root for native RPM/DNF validation, built rootlessly.
#
# Usage: tests/sandbox/fedora-userns.sh WORKDIR STATIC_PKGX [RELEASE]
#
# A disposable Arch tool root provides dnf5 (Arch-signed packages). The initial
# Fedora package set is fetched over TLS from dl.fedoraproject.org without RPM
# signature checks, because the Fedora keys are themselves part of that set;
# every later transaction inside the root uses the installed fedora-repos
# configuration with gpgcheck enabled. Everything runs in a user namespace with
# the subordinate UID range (unshare --map-auto); host package databases are never
# targets. The Fedora root lives on a tmpfs inside the namespace and disappears
# with it. CHECK_DIR (default tests/containers) supplies the check scripts.
set -eu
WORK=${1:?usage: fedora-userns.sh WORKDIR STATIC_PKGX [RELEASE]}
PKGX=${2:?usage: fedora-userns.sh WORKDIR STATIC_PKGX [RELEASE]}
RELEASE=${3:-44}
CHECKS=${CHECKS:-rpm.sh dnf-transactions.sh}
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
    dnf5 libgomp ca-certificates coreutils
NAME=$(basename "$(mktemp -d "$TOOLS/srv/fedora$RELEASE.XXXXXX")")
export TOOLS NAME RELEASE CHECKS PKGX CHECK_DIR
full sh -eu <<'INNER'
mount -t tmpfs -o mode=0755 tmpfs "$TOOLS/srv/$NAME"
mount --rbind /dev "$TOOLS/dev"
mount -t proc proc "$TOOLS/proc"
cp /etc/resolv.conf "$TOOLS/etc/resolv.conf"
repo="https://dl.fedoraproject.org/pub/fedora/linux/releases/$RELEASE/Everything/x86_64/os/"
chroot "$TOOLS" /usr/bin/env -i PATH=/usr/bin LC_ALL=C \
    dnf5 --no-plugins --installroot="/srv/$NAME" --releasever="$RELEASE" --use-host-config \
    --repofrompath="bootstrap,$repo" --repo=bootstrap \
    --setopt=bootstrap.gpgcheck=0 --setopt=install_weak_deps=0 \
    --assumeyes --quiet install \
    fedora-release fedora-repos fedora-gpg-keys dnf5 rpm bash coreutils \
    sudo shadow-utils grep sed
FED="$TOOLS/srv/$NAME"
mkdir -p "$FED/checks"
cp "$PKGX" "$FED/usr/local/bin/pkgx"
mount --bind "$CHECK_DIR" "$FED/checks"
mount --rbind /dev "$FED/dev"
mount -t proc proc "$FED/proc"
cp /etc/resolv.conf "$FED/etc/resolv.conf"
for check in $CHECKS; do
    echo "== $check"
    chroot "$FED" /usr/bin/env -i \
        PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin \
        HOME=/root LC_ALL=C sh "/checks/$check"
done
INNER
rmdir "$TOOLS/srv/$NAME"
echo 'disposable root removed with its namespace'
