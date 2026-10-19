# ADR 009: Conservative cleanup classes and execution

Status: implemented experimentally; injected-runner regressions and the disposable
Arch root (docs/validation.md). APT cleanup has injected evidence only.

`xpac audit` (and `clean --dry-run`) separate findings into three classes:

- `safe_native_cleanup`: native package caches. Only the directories the package
  manager reports are measured (`apt-config shell ARCHIVES Dir::Cache::archives/d`,
  `pacman-conf CacheDir`), counting regular package files directly inside them
  (`*.deb`, `*.pkg.tar*`); symlinks, subdirectories and other files are ignored.
  Reclaimable space is the measured size.
- `suggested_cleanup`: native orphan candidates (APT autoremove simulation, pacman
  `-Qdtq`, Homebrew dependency leaves). Native orphan status is not proof that the
  user no longer needs a package.
- `manual_review`: dpkg residual-configuration records. No purge is offered.

Execution never deletes a path itself:

- `clean --cache --provider P [--yes]` previews the measurement and exact command,
  requires confirmation, re-measures and refuses a stale preview, then runs only
  `apt-get clean` or `pacman --sync --clean --noconfirm --noprogressbar --color never`.
  Afterwards the cache must not have grown, and APT's must be empty. Intent and
  result are journaled. `pacman -Sc` keeps installed versions and removes unused
  sync databases; older versions can no longer be reinstalled from cache.
- `clean --orphans --provider P [--yes]` turns the current orphan candidates into an
  ordinary removal request for the provider's reviewed executor (ADR 002/007): the
  same preview, protections, confirmation, revalidation, journal and verification.
  Providers without an executor refuse.

Both target exactly one provider. `--sudo` delegates only the two exact cleanup
vectors (and existing transaction templates) to the fixed system binaries.

Not implemented: Homebrew/DNF/Flatpak cache cleanup, residual-configuration purge,
orphan-space estimates and any filesystem cleanup outside native commands.
