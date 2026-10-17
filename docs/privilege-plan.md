# Explicit APT privilege delegation

Implement --sudo only for confirmed APT execution. Inspection and simulation
remain unprivileged. Authenticate through sudo's own terminal UI (never capture a
password), then repeat native plan/inventory validation. The eventual transaction
uses noninteractive sudo so credential expiry cannot introduce another prompt.

Use fixed /usr/bin/sudo and /usr/bin/apt-get paths; refuse a different apt-get on
PATH or native database/config environment overrides. Do not elevate pkgx itself,
its journal, manifest parsing, registry writes, or arbitrary CommandSpec programs.
TUI authentication must temporarily restore the normal terminal before prompting.

Tests use an injected authenticator/runner and harmless child processes only.
Do not execute sudo or a native transaction on the workstation. Native PAM/sudo
policy and terminal behavior require disposable Debian/Ubuntu acceptance evidence.
