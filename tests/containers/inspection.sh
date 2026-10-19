#!/bin/sh
# Run ONLY inside the disposable CI containers documented in the workflow.
set -eu
xpac --version
xpac providers --json
xpac deps apt:bash --json
xpac doctor --json
xpac list --provider apt --json > /tmp/inventory.json
xpac search bash --provider apt --json
xpac info apt:bash --json
xpac why apt:bash --json
xpac install apt:hello --dry-run --json
xpac export -o /tmp/system.toml
xpac diff -f /tmp/system.toml --json
xpac plan -f /tmp/system.toml --json
# Reapplying a complete export is a read-only no-op.
xpac apply -f /tmp/system.toml --yes --json
xpac audit --provider apt --json
xpac clean --provider apt --dry-run --json
xpac history --state-dir /tmp/xpac-journal --json
