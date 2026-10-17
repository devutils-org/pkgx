#!/bin/sh
# NEVER run on a workstation: isolated root-owned disposable APT container only.
set -eu
apt-get install -y sudo passwd
useradd -m -s /bin/sh pkgx-check
status=$(dpkg-query -W -f='${db:Status-Status}' hello 2>/dev/null || true)
[ "$status" != installed ]
# Inspection and simulation must not require a sudoers grant or credentials.
sudo -u pkgx-check -H -- /usr/local/bin/pkgx list --provider apt --json
sudo -u pkgx-check -H -- /usr/local/bin/pkgx install apt:hello --sudo --dry-run --json
if sudo -u pkgx-check -H -- /usr/local/bin/pkgx install apt:hello --sudo --yes --state-dir /tmp/pkgx-sudo-records --json; then
    echo 'unauthorized sudo execution unexpectedly succeeded' >&2
    exit 1
fi
[ ! -e /tmp/pkgx-sudo-records ]
status=$(dpkg-query -W -f='${db:Status-Status}' hello 2>/dev/null || true)
[ "$status" != installed ]
# This policy exists only inside the disposable test container.
printf '%s\n' 'pkgx-check ALL=(root) NOPASSWD: /usr/bin/apt-get' > /etc/sudoers.d/pkgx-check
chmod 0440 /etc/sudoers.d/pkgx-check
sudo -u pkgx-check -H -- /usr/local/bin/pkgx install apt:hello --sudo --yes --state-dir /tmp/pkgx-sudo-records --json
[ "$(dpkg-query -W -f='${db:Status-Status}' hello)" = installed ]
if sudo -u pkgx-check -H -- /usr/local/bin/pkgx remove apt:hello --yes --state-dir /tmp/pkgx-sudo-records --json; then
    echo 'unprivileged package removal unexpectedly succeeded' >&2
    exit 1
fi
[ "$(dpkg-query -W -f='${db:Status-Status}' hello)" = installed ]
sudo -u pkgx-check -H -- /usr/local/bin/pkgx remove apt:hello --sudo --yes --state-dir /tmp/pkgx-sudo-records --json
status=$(dpkg-query -W -f='${db:Status-Status}' hello 2>/dev/null || true)
[ "$status" != installed ]
[ "$(stat -c %U /tmp/pkgx-sudo-records)" = pkgx-check ]
[ "$(stat -c %a /tmp/pkgx-sudo-records)" = 700 ]
for record in /tmp/pkgx-sudo-records/*.json; do
    [ "$(stat -c %U "$record")" = pkgx-check ]
    [ "$(stat -c %a "$record")" = 600 ]
done
