#!/bin/sh
# NEVER run on a workstation: disposable root only (tests/sandbox/flatpak-userns.sh).
# Builds a tiny runtime and app into a local, unsigned test repository; no Flatpak
# remote is contacted. Export triggers may warn inside the namespace.
set -eu
fail() { echo "$1" >&2; exit 1; }
APP=app/org.xpac.Hello/x86_64/stable
installed() { flatpak list --user --all --columns=ref:full | grep -qx "${1#*/}"; }
mkdir -p /tmp/rt/files/bin /tmp/rt/usr /tmp/app/files/bin /tmp/app/export
printf '[Runtime]\nname=org.xpac.Platform\nruntime=org.xpac.Platform/x86_64/1\nsdk=org.xpac.Platform/x86_64/1\n' > /tmp/rt/metadata
printf '[Application]\nname=org.xpac.Hello\nruntime=org.xpac.Platform/x86_64/1\ncommand=hello\n' > /tmp/app/metadata
printf '#!/bin/sh\necho hello\n' > /tmp/app/files/bin/hello
chmod +x /tmp/app/files/bin/hello
flatpak build-export --runtime /tmp/repo /tmp/rt 1 >/dev/null
flatpak build-export /tmp/repo /tmp/app stable >/dev/null
flatpak remote-add --user --no-gpg-verify local /tmp/repo
xpac install "flatpak:$APP:user" --dry-run --json
if printf 'no\n' | xpac install "flatpak:$APP:user"; then
    fail 'unconfirmed install unexpectedly ran'
fi
! installed "$APP" || fail 'unconfirmed install changed the system'
cat > /tmp/app.toml <<MANIFEST
version = 1
[packages.flatpak]
install = ['$APP:user']
MANIFEST
xpac apply -f /tmp/app.toml --yes --state-dir /tmp/xpac-records --json
installed "$APP" || fail 'app not installed'
installed runtime/org.xpac.Platform/x86_64/1 || fail 'runtime not installed'
xpac apply -f /tmp/app.toml --yes --json | grep -q '"operations":\[\]'
if xpac remove flatpak:runtime/org.xpac.Platform/x86_64/1:user --dry-run; then
    fail 'runtime removal preview unexpectedly succeeded'
fi
# A second remote offering the same ref makes the origin ambiguous.
flatpak remote-add --user --no-gpg-verify mirror /tmp/repo
xpac remove "flatpak:$APP:user" --yes --state-dir /tmp/xpac-records --json
installed runtime/org.xpac.Platform/x86_64/1 || fail 'removal took the runtime'
if xpac install "flatpak:$APP:user" --yes; then
    fail 'ambiguous origin unexpectedly installed'
fi
! installed "$APP" || fail 'ambiguous install changed the system'
xpac history --state-dir /tmp/xpac-records --json > /tmp/history.json
[ "$(grep -o '"success":true' /tmp/history.json | wc -l)" -ge 2 ] || fail 'journal is missing results'
echo 'flatpak checks passed'
