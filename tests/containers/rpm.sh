#!/bin/sh
# Read-only native checks, invoked only inside the disposable Fedora CI container.
set -eu
pkgx providers --json
pkgx deps dnf:bash --json
pkgx list --provider dnf --json
pkgx info dnf:bash --json
pkgx why dnf:bash --json
pkgx audit --provider dnf --json
pkgx export --provider dnf -o /tmp/fedora.toml
pkgx diff -f /tmp/fedora.toml --json
pkgx plan -f /tmp/fedora.toml --json
