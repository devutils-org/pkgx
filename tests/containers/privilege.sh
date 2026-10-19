#!/bin/sh
# NEVER run on a workstation: isolated root-owned disposable APT container only.
set -eu
apt-get install -y sudo passwd
useradd -m -s /bin/sh xpac-check
status=$(dpkg-query -W -f='${db:Status-Status}' hello 2>/dev/null || true)
[ "$status" != installed ]
# Inspection and simulation must not require a sudoers grant or credentials.
sudo -u xpac-check -H -- /usr/local/bin/xpac list --provider apt --json
sudo -u xpac-check -H -- /usr/local/bin/xpac install apt:hello --sudo --dry-run --json
if sudo -u xpac-check -H -- /usr/local/bin/xpac install apt:hello --sudo --yes --state-dir /tmp/xpac-sudo-records --json; then
    echo 'unauthorized sudo execution unexpectedly succeeded' >&2
    exit 1
fi
[ ! -e /tmp/xpac-sudo-records ]
status=$(dpkg-query -W -f='${db:Status-Status}' hello 2>/dev/null || true)
[ "$status" != installed ]
# This policy exists only inside the disposable test container.
printf '%s\n' 'xpac-check ALL=(root) NOPASSWD: /usr/bin/apt-get' > /etc/sudoers.d/xpac-check
chmod 0440 /etc/sudoers.d/xpac-check
sudo -u xpac-check -H -- /usr/local/bin/xpac install apt:hello --sudo --yes --state-dir /tmp/xpac-sudo-records --json
[ "$(dpkg-query -W -f='${db:Status-Status}' hello)" = installed ]
if sudo -u xpac-check -H -- /usr/local/bin/xpac remove apt:hello --yes --state-dir /tmp/xpac-sudo-records --json; then
    echo 'unprivileged package removal unexpectedly succeeded' >&2
    exit 1
fi
[ "$(dpkg-query -W -f='${db:Status-Status}' hello)" = installed ]
sudo -u xpac-check -H -- /usr/local/bin/xpac remove apt:hello --sudo --yes --state-dir /tmp/xpac-sudo-records --json
status=$(dpkg-query -W -f='${db:Status-Status}' hello 2>/dev/null || true)
[ "$status" != installed ]
[ "$(stat -c %U /tmp/xpac-sudo-records)" = xpac-check ]
[ "$(stat -c %a /tmp/xpac-sudo-records)" = 700 ]
for record in /tmp/xpac-sudo-records/*.json; do
    [ "$(stat -c %U "$record")" = xpac-check ]
    [ "$(stat -c %a "$record")" = 600 ]
done
