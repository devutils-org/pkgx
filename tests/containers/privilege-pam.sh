#!/bin/sh
# NEVER run on a workstation: disposable Debian/Ubuntu root only. Drives the
# interactive --sudo path through a pseudo-terminal: xpac confirmation first,
# then sudo's own PAM password prompt, with a throwaway account and password.
set -eu
apt-get install -y -qq sudo passwd python3 >/dev/null
id xpac-pam >/dev/null 2>&1 || useradd -m -s /bin/sh xpac-pam
echo 'xpac-pam:xpac-disposable-only' | chpasswd
# Password required (no NOPASSWD): only apt-get may be run as root.
printf '%s\n' 'xpac-pam ALL=(root) /usr/bin/apt-get' > /etc/sudoers.d/xpac-pam
chmod 0440 /etc/sudoers.d/xpac-pam
status() { dpkg-query -W -f='${db:Status-Status}' hello 2>/dev/null || true; }
[ "$(status)" != installed ] || apt-get remove -y -qq hello >/dev/null
cat > /tmp/pam-drive.py <<'PY'
import os, pty, pwd, sys, time
password, action = sys.argv[1], sys.argv[2]
user = pwd.getpwnam("xpac-pam")
pid, fd = pty.fork()
if pid == 0:
    # Keep the pty as controlling terminal (util-linux su -c would setsid).
    os.initgroups(user.pw_name, user.pw_gid)
    os.setgid(user.pw_gid)
    os.setuid(user.pw_uid)
    env = {"PATH": "/usr/bin:/bin", "HOME": user.pw_dir, "LC_ALL": "C", "USER": user.pw_name}
    os.execve("/usr/local/bin/xpac", ["xpac", action, "apt:hello", "--sudo",
              "--state-dir", "/tmp/pam-records"], env)
seen, answered, deadline = b"", set(), time.time() + 300
while time.time() < deadline:
    try:
        chunk = os.read(fd, 4096)
    except OSError:
        break
    if not chunk:
        break
    seen += chunk
    if b"Type yes:" in seen and "confirm" not in answered:
        answered.add("confirm"); os.write(fd, b"yes\n")
    if b"password for xpac-pam" in seen.lower() and "password" not in answered:
        answered.add("password"); os.write(fd, password.encode() + b"\n")
    if seen.lower().count(b"password for xpac-pam") > 1 and "retry" not in answered:
        answered.add("retry"); os.write(fd, b"\x03")  # refuse retries after a wrong password
_, code = os.waitpid(pid, 0)
sys.stdout.write(seen.decode(errors="replace"))
sys.exit(os.waitstatus_to_exitcode(code))
PY
if python3 /tmp/pam-drive.py wrong-password install; then
    echo 'wrong sudo password unexpectedly authorized' >&2
    exit 1
fi
[ "$(status)" != installed ] || { echo 'refused authentication changed state' >&2; exit 1; }
python3 /tmp/pam-drive.py xpac-disposable-only install
[ "$(status)" = installed ] || { echo 'interactive sudo install failed' >&2; exit 1; }
sudo -k -u xpac-pam 2>/dev/null || true
python3 /tmp/pam-drive.py xpac-disposable-only remove
[ "$(status)" != installed ] || { echo 'interactive sudo removal failed' >&2; exit 1; }
[ "$(stat -c %U /tmp/pam-records)" = xpac-pam ]
echo 'interactive sudo checks passed'
