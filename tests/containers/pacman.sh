#!/bin/sh
# Read-only native checks, invoked only inside the disposable Arch CI container.
set -eu
pkgx providers --json
pkgx deps pacman:bash --json
pkgx list --provider pacman --json
pkgx info pacman:bash --json
pkgx why pacman:bash --json
pkgx audit --provider pacman --json
pkgx export --provider pacman -o /tmp/arch.toml
pkgx diff -f /tmp/arch.toml --json
pkgx plan -f /tmp/arch.toml --json
