#!/bin/sh
# Read-only native checks, invoked only inside the disposable Arch CI container.
set -eu
xpac providers --json
xpac deps pacman:bash --json
xpac list --provider pacman --json
xpac info pacman:bash --json
xpac why pacman:bash --json
xpac audit --provider pacman --json
xpac export --provider pacman -o /tmp/arch.toml
xpac diff -f /tmp/arch.toml --json
xpac plan -f /tmp/arch.toml --json
