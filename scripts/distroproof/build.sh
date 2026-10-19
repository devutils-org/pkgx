#!/bin/sh
# Build on the host; no package-manager commands, sudo or guest scripts here.
set -eu
cd "$(dirname "$0")/../.."
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/xpac-target}"
# zigbuild fixes the glibc symbol floor independently of the build machine.
cargo zigbuild --release --locked --offline --target x86_64-unknown-linux-gnu.2.17 -p xpac-cli
cargo build --release --locked --offline --target x86_64-unknown-linux-musl -p xpac-cli
python3 scripts/distroproof/package.py
