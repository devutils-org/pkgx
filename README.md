<p align="center"><code>devutils / RECONCILE</code></p>

<h1 align="center">xpac</h1>

<p align="center">Know what is installed. Review what changes.</p>

<p align="center">
  <a href="https://devutils-org.github.io/tools/xpac/">Website</a> ·
  <a href="#documentation">Documentation</a> ·
  <a href="https://github.com/devutils-org">devutils</a>
</p>

## Overview

- Inspect native package inventory, provenance and dependencies across five providers.
- Reconcile packages and home-relative configuration files with a TOML manifest.
- Review plans before execution; inspect packages in the CLI or TUI.

Native execution and managed files are experimental. Changes require confirmation;
partial failures do not imply rollback. See [support](docs/support.md) and
[recovery](docs/recovery.md).

Linux x86_64 portability is checked with installed GNU/musl releases across
16 distro images plus root and lockdown variants. See the
[measured matrix and limits](docs/distroproof.md).

## Install

Download an archive from the [releases](https://github.com/devutils-org/xpac/releases)
(Linux x86_64 GNU or static musl, macOS arm64), verify it, then install the binary:

```sh
curl -fLO https://github.com/devutils-org/xpac/releases/download/v0.1.0/xpac-v0.1.0-x86_64-unknown-linux-musl.tar.gz && \
gh attestation verify xpac-v0.1.0-x86_64-unknown-linux-musl.tar.gz --repo devutils-org/xpac && \
tar xzf xpac-v0.1.0-x86_64-unknown-linux-musl.tar.gz && \
mkdir -p ~/.local/bin && \
install -m 755 xpac-v0.1.0-x86_64-unknown-linux-musl/xpac ~/.local/bin/
```

Or from a source checkout with Rust stable:

```sh
cargo build --release --locked -p xpac-cli
mkdir -p ~/.local/bin
install -m 755 target/release/xpac ~/.local/bin/
```

Keep `~/.local/bin` on your `PATH`. Inspection needs the matching native provider;
`demo` needs none. See [installation and archives](docs/install.md).

## Use

```sh
xpac demo                              # synthetic data, no system changes
xpac list --provider apt                # installed native packages
xpac why apt:curl:amd64                 # installation reason
xpac plan -f examples/system.toml       # review a manifest
xpac apply -f examples/system.toml --dry-run
```

APT, Pacman, RPM/DNF, Flatpak, Homebrew, apk and Portage consultation are implemented.
Execution support differs by provider: see the [backend matrix](docs/backends.md).

## Documentation

[CLI and behavior reference](REFERENCE.md) · [Manifests](docs/manifest.md) ·
[Linux portability](docs/distroproof.md) · [Recipes](docs/recipes.md) · [Validation](docs/validation.md) · [Roadmap](docs/ROADMAP.md) ·
[All documentation](docs/README.md)

## Development

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Use disposable environments for native transactions; see [validation](docs/validation.md).
To try every feature by hand, follow the [testing guide](docs/how-to-test.md) (French).

## License

[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
