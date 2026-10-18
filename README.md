<p align="center"><code>devutils / RECONCILE</code></p>

<h1 align="center">pkgx</h1>

<p align="center">Know what is installed. Review what changes.</p>

<p align="center">
  <a href="https://devutils-org.github.io/tools/pkgx/">Website</a> ·
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

## Install

From a source checkout with Rust stable:

```sh
cargo build --release --locked -p pkgx-cli
mkdir -p ~/.local/bin
install -m 755 target/release/pkgx ~/.local/bin/
```

Keep `~/.local/bin` on your `PATH`. Inspection needs the matching native provider;
`demo` needs none. See [installation and archives](docs/install.md).

## Use

```sh
pkgx demo                              # synthetic data, no system changes
pkgx list --provider apt                # installed native packages
pkgx why apt:curl:amd64                 # installation reason
pkgx plan -f examples/system.toml       # review a manifest
pkgx apply -f examples/system.toml --dry-run
```

APT, Pacman, RPM/DNF, Flatpak and Homebrew consultation are implemented.
Execution support differs by provider: see the [backend matrix](docs/backends.md).

## Documentation

[CLI and behavior reference](REFERENCE.md) · [Manifests](docs/manifest.md) ·
[Recipes](docs/recipes.md) · [Validation](docs/validation.md) · [Roadmap](pkgx.md)

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
