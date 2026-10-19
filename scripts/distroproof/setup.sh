#!/bin/sh
# Distroproof setup: root INSIDE the disposable VM. Never invoke on the host.
set -eu
[ -n "${DP_TARGET:-}" ] && [ -n "${DP_IMAGE:-}" ] && [ "$PWD" = /distroproof/work ] || {
    echo 'setup requires a distroproof guest working directory and target' >&2; exit 1;
}
# cloud-init images normally have Python; Alpine's small image needs it.
if ! command -v python3 >/dev/null 2>&1; then
    if [ -x /usr/libexec/platform-python ]; then
        mkdir -p /usr/local/bin
        ln -s /usr/libexec/platform-python /usr/local/bin/python3
    elif command -v apk >/dev/null 2>&1; then apk add --no-cache python3
    elif command -v apt-get >/dev/null 2>&1; then apt-get update && apt-get install -y python3
    else echo 'guest needs Python 3.6+ to run the matrix checks' >&2; exit 1
    fi
fi
python3 - <<'PY'
import hashlib, json
from pathlib import Path
builds = json.loads(Path('dist/distroproof/builds.json').read_text())
for flavor, info in builds.items():
    path = Path('dist/distroproof/' + flavor + '.tar.gz')
    if hashlib.sha256(path.read_bytes()).hexdigest() != info['archive_sha256']:
        raise ValueError('archive checksum mismatch: ' + flavor)
PY
for flavor in gnu musl; do
    mkdir -p "/opt/xpac/$flavor"
    tar -xzf "dist/distroproof/$flavor.tar.gz" -C "/opt/xpac/$flavor" --strip-components=1
done
mkdir -p /usr/local/bin
ln -sf /opt/xpac/musl/xpac /usr/local/bin/xpac
# Alpine and Gentoo images carry no package index: refreshed once, as a user
# does after installing, so previews resolve. No package changes here.
case "$DP_IMAGE" in
    alpine) apk update ;;
    # The stage's binary host is configured but not enabled, as a user who
    # wants binaries enables it.
    # A mirror caught mid-update fails Portage's manifest verification
    # ("Manifest mismatch for lastreplicate"): the sync is tried again.
    gentoo) emerge --sync --quiet || { sleep 30; emerge --sync --quiet; } || { sleep 60; emerge --sync --quiet; }
            printf '%s\n' 'FEATURES="${FEATURES} getbinpkg"' >> /etc/portage/make.conf ;;
esac
# Bind subsequent mutating checks to the guest setup and its target.
printf '%s\n' "$DP_TARGET" > /etc/xpac-distroproof-guest
if [ "${DP_TARGET#transactions-}" != "$DP_TARGET" ]; then
    # Buster is archived: keep signature verification, use its signed archive.
    if [ "$DP_IMAGE" = debian-10 ]; then
        printf '%s\n' 'deb http://archive.debian.org/debian buster main' \
            'deb http://archive.debian.org/debian-security buster/updates main' > /etc/apt/sources.list
    fi
    case "$DP_IMAGE" in
        debian-*|ubuntu-*) apt-get update; apt-get install -y sudo ;;
        arch) pacman -Sy --noconfirm ;;
        alpine|gentoo) : ;; # indexes refreshed below for every cell.
        fedora-*) : ;; # dnf5 fetches metadata inside its own checks.
        *) echo "unsupported transaction image: $DP_IMAGE" >&2; exit 1 ;;
    esac
fi
