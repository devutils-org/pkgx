#!/bin/sh
# Read-only native checks, invoked only inside the disposable Fedora CI container.
set -eu
xpac providers --json
xpac deps dnf:bash --json
xpac list --provider dnf --json
xpac info dnf:bash --json
xpac why dnf:bash --json
xpac audit --provider dnf --json
xpac export --provider dnf -o /tmp/fedora.toml
xpac diff -f /tmp/fedora.toml --json
xpac plan -f /tmp/fedora.toml --json
