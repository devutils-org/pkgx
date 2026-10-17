#!/bin/sh
# Run ONLY inside the disposable CI containers documented in the workflow.
set -eu
pkgx --version
pkgx providers --json
pkgx deps apt:bash --json
pkgx doctor --json
pkgx list --provider apt --json > /tmp/inventory.json
pkgx search bash --provider apt --json
pkgx info apt:bash --json
pkgx why apt:bash --json
pkgx install apt:hello --dry-run --json
pkgx export -o /tmp/system.toml
pkgx diff -f /tmp/system.toml --json
pkgx plan -f /tmp/system.toml --json
# Reapplying a complete export is a read-only no-op.
pkgx apply -f /tmp/system.toml --yes --json
pkgx audit --provider apt --json
pkgx clean --provider apt --dry-run --json
pkgx history --state-dir /tmp/pkgx-journal --json
