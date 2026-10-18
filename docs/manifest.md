# Manifest v1

```toml
version = 1
[system]
name = "workstation"
mode = "additive"
[packages.apt]
install = ["git", "curl:amd64"]

[platforms.macos.packages.brew]
install = ["ripgrep:formula"]

[profiles.base.packages.apt]
install = ["build-essential"]
[profiles.dev]
extends = ["base"]
[profiles.dev.packages.apt]
install = ["gdb"]
```

Use `pkgx diff -f system.toml --profile dev` to activate a profile. Profiles are never
automatically activated. Resolution is an additive union of base packages, matching
platform packages, then requested profiles with their ancestors first in the declared
`extends` order. Requested profiles follow CLI order; each profile is included once.
Package names are sorted/deduplicated across layers. A duplicate within one layer is
an error. There are no implicit removals or scalar overrides in package layers.

Only `linux` and `macos` platform keys are accepted. Excluded platform sections are
reported in resolution notes. Every layer is validated even when inactive; unknown
fields, providers, profiles, inheritance cycles and depth above 64 fail. At most 1 MiB
of manifest data is accepted. Schema parse errors report a line without echoing input.

Provider sections accept `install`, with names already scoped by the provider key.
See [identities and capabilities](backends.md). APT example names omit `apt:`. Brew
names include `:formula` or `:cask`; Flatpak names include a full kind/ref and scope.
Scripts are configured through a separate recipe schema and cannot appear in package
sections. Selectors, globs and executable/interpolated fields are rejected; files use the
separate `files` table below.
Search commands alone accept native regex patterns.

`system`, name, mode, packages, platforms and profiles are optional. Mode defaults to
additive. Strict mode is limited to explicitly granted APT package identities;
native manual marks alone never authorize undeclared removals. See the strict-mode
workflow below.

`export --provider apt --provider pacman` includes installed identities with scope.
It is an inventory snapshot, not a reproducible lockfile or a management claim.
Multiple versions of one identity collapse to one presence requirement. Writes to
`-o` are staged atomically, with no destination-symlink following or early truncation.

`diff` queries every selected provider and reports missing/present/undeclared IDs.
Unavailable providers fail explicitly. `plan -f` assembles native plans in stable
provider order; unsupported missing-package operations fail. `apply -f` checks all
provider inventories first and refuses if any missing identity has no executor,
before changing any provider. APT and Pacman can execute; each provider is a
separate reviewed transaction in stable order, stopping at the first failure. A complete mixed
manifest is a successful no-op. Existing versions satisfy presence; version sync is
not supported. Additive mode retains all undeclared software. Strict mode retains
all ungranted identities and refuses native removal cascades outside its scope.

## Configuration files (experimental)

Home-relative configuration files can be declared in the base, platform and profile
layers (ADR 013). Sources are relative to the manifest directory, must be regular
files without symlinks and are limited to 1 MiB each (16 MiB in total). `mode`
accepts 0600 (default), 0644, 0700 or 0755. Identical declarations across layers
collapse; differing ones are an error.

```toml
[files."~/.tmux.conf"]
source = "dotfiles/tmux.conf"
mode = "0644"
```

`diff` reports each target as missing, in sync, outdated, drifted, unmanaged
conflict, unsupported or undeclared, without creating state or touching HOME.
`plan -f` shows a unified diff (contents are displayed) for missing and outdated
targets; drifted, unmanaged, or unsupported targets fail the plan by name. `apply -f`
writes files atomically as your user after package transactions, with a separate
confirmation, and records receipts in `<state-dir>/files`. Reload actions such as
`tmux source-file` are never run.

An existing file is never overwritten until you adopt it; adoption keeps a private
backup under `<state-dir>/files/backups` and the next apply replaces it after review:

```sh
pkgx file adopt ~/.tmux.conf --dry-run
pkgx file adopt ~/.tmux.conf
pkgx apply -f system.toml
pkgx file list
pkgx file forget ~/.tmux.conf   # drops the receipt, keeps the file
pkgx file remove ~/.tmux.conf --dry-run
```

Removing a declaration keeps the file and its receipt (reported as undeclared).
Only `pkgx file remove` deletes a file, and only a managed file still identical to its
receipt, after preview, confirmation and a private backup; drifted or unmanaged
files are never deleted. `export` never includes files. `pkgx file list`
reports each receipt as intact, drifted, missing or unsupported without a manifest.
The TUI manifest review (m) shows file states read-only; only `apply -f` writes.

Unknown schema versions are rejected. Future incompatible changes require a new
version and explicit migration preserving the original; no migration is implicit.

## Explicit strict scope (experimental APT)

First review and grant individual, concrete installed package identities. This
changes only pkgx state and does not install or remove native software:

```sh
pkgx manage apt:hello:amd64 --dry-run
pkgx manage apt:hello:amd64
pkgx managed --json
```

Use the real architecture of the installed package. Confirmation is interactive
`yes`, or an explicit --yes in automation. A strict manifest such as
`examples/strict.toml` removes only granted, undeclared identities in its explicit
APT section. Review it using diff and plan before apply:

```sh
pkgx diff -f examples/strict.toml
pkgx plan -f examples/strict.toml
pkgx apply -f examples/strict.toml --dry-run
pkgx apply -f examples/strict.toml
```

Changing the recorded native version or reason invalidates the grant for removal.
A native cascade that would remove any package outside the proposed strict removal
set fails before execution, including a granted package still desired by the
manifest. Holds and critical-package protections also apply. With no grants, strict
mode retains all undeclared software. Omitting an APT section never requests its
removal. New installations are not adopted automatically; grant them separately.

`manage apt:hello:amd64 --forget` revokes a grant without uninstalling anything and
works even after the package is gone. Reads/previews create no state directory.
The registry is private, versioned and locked during strict execution. Historical
grants may remain after removal; they are not a claim that the package is still
installed. Use the same --state-dir for grant, inspection and apply. Strict mode is
also available in TUI manifest review, with the same core checks and typed apply
confirmation. See [the management policy](adr/005-managed-strict-scope.md).
