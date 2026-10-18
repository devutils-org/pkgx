# pkgx

Rust CLI for native package inventory, provenance, conservative plans and declarative
TOML reconciliation of packages and home-relative configuration files. Native APT execution is experimental; platform CI is configured
but has not run in this development environment.

```sh
cargo run -p pkgx-cli -- providers
cargo run -p pkgx-cli -- doctor --json
cargo run -p pkgx-cli -- list --provider apt --json
cargo run -p pkgx-cli -- search 'curl' --provider apt
cargo run -p pkgx-cli -- search curl --provider dnf --installed
cargo run -p pkgx-cli -- info apt:curl
cargo run -p pkgx-cli -- why apt:curl:amd64
cargo run -p pkgx-cli -- deps apt:curl:amd64
cargo run -p pkgx-cli -- install apt:curl --dry-run
cargo run -p pkgx-cli -- remove apt:curl --dry-run
cargo run -p pkgx-cli -- export --provider apt -o system.toml
cargo run -p pkgx-cli -- diff -f examples/profiles.toml --profile dev
cargo run -p pkgx-cli -- plan -f system.toml
cargo run -p pkgx-cli -- plan apt:curl --save preview.json --expires-in 300
cargo run -p pkgx-cli -- apply --plan preview.json --dry-run
cargo run -p pkgx-cli -- apply -f system.toml --dry-run
cargo run -p pkgx-cli -- audit --provider apt
cargo run -p pkgx-cli -- clean --provider apt --dry-run
cargo run -p pkgx-cli -- history --limit 20 --offset 0 --json
cargo run -p pkgx-cli -- tui --provider apt
cargo run -p pkgx-cli -- file adopt ~/.tmux.conf --dry-run
cargo run -p pkgx-cli -- file list
cargo run -p pkgx-cli -- recipe check -f tests/fixtures/recipe.toml
cargo run -p pkgx-cli -- completions bash
```

Use --profile only when the manifest defines that profile. `examples/system.toml`
is a minimal additive example. Profiles/platform layers compose deterministically;
unknown fields and incompatible operations fail before execution. Additive mode retains undeclared software. Experimental strict mode can remove
only explicitly granted APT identities; it refuses native cascades outside that scope. Export writes atomically; an exported inventory is not a lockfile.

Implemented native consultation: APT, Pacman, RPM/DNF, default Flatpak user/system,
and Homebrew formulae/casks. Identities preserve provider, architecture and scope.
APT supports native simulation and experimental execution. Pacman supports an
explicitly uncertain --print preview and experimental confirmed execution (ADR 007);
DNF5 previews a stored resolved transaction and executes with pinned NEVRAs (ADR 010);
Flatpak previews the single offering remote, commit and runtime, and executes with
--no-related (ADR 011); Homebrew formulae use `--dry-run` previews and verified
installs/removals (ADR 012).
Other native solvers/executors are unsupported;
there is no implicit provider fallback. See [backend matrix](docs/backends.md).

`install`, `remove` and `apply` execute native packages (APT, Pacman, DNF, Flatpak,
Homebrew formulae) only after a preview and interactive `yes`,
or explicit --yes. Noninteractive execution without --yes is refused. Revalidation
follows confirmation. Execution requires native privileges; explicit --sudo can
delegate the reviewed APT/Pacman transaction without elevating inspection or pkgx state.
A private transaction journal records intent and verified results. Saved plans expire
and must match a fresh native plan before execution; see [saved-plan policy](docs/adr/004-saved-plan-review.md). SIGINT/SIGTERM
cancels subprocess work on Unix. Partial failures never imply rollback.

Audit and `clean --dry-run` classify findings as safe native cleanup (measured APT/Pacman
package caches), suggested cleanup (native orphans) and manual review (dpkg residual
records). `clean --cache` runs only `apt-get clean`/`pacman -Sc` after confirmation;
`clean --orphans` sends orphan candidates through the reviewed removal executor
(ADR 009). pkgx never deletes configurations, home data or unmanaged paths itself. The Ratatui frontend supports filtering, info, provenance, orphan inspection and
plan review. It is read-only by default. `tui --allow-apply` enables experimental
APT operations after pressing x and typing apply, with core revalidation, journal
and cancellable worker progress.

Manifests can also declare configuration files (experimental, ADR 013):

```toml
[packages.apt]
install = ["tmux"]

[files."~/.tmux.conf"]
source = "dotfiles/tmux.conf"   # relative to the manifest
mode = "0644"
```

`diff` reports each file as missing, in sync, outdated, drifted, unmanaged conflict,
unsupported or undeclared. `plan -f` shows a unified diff; `apply -f` writes files
atomically as your user after package transactions, with its own confirmation, and
records private receipts. pkgx copies bytes only: no templates, merging, system paths
or reload actions. Existing files are never overwritten until `pkgx file adopt`
backs them up and records them; local edits to managed files are reported as drift
and refused. pkgx never deletes configuration files (`file forget` drops a receipt
only). See the [manifest reference](docs/manifest.md#configuration-files-experimental).

Recipe check/verify validate pinned integrity metadata and optionally verify a bounded
HTTPS download in private temporary storage without executing it. `recipe install`
runs a verified script as your user only after trust confirmation, for home-relative
declared resources that did not exist before, and records a receipt; `recipe
uninstall` runs the recipe's own uninstall action only while owned files still match
their receipt (ADR 008). pkgx never deletes resource paths itself.

JSON success output goes to stdout with schema_version 1. New results use `data`;
list/providers/doctor retain their original top-level fields. Runtime errors go to
stderr (JSON with --json) and exit 1. Clap syntax errors use text and exit 2. Execution
previews/prompts go to stderr; stdout contains the final record. Export/completions
emit their native text format even with --json. TUI rejects --json.

Subprocesses use arguments, closed stdin, C locale, bounded waits and an 8 MiB limit
per output stream. Native stderr is not echoed; raw command arguments/environment
are not logged. PATH and package-manager configuration must be trusted. Inspection
needs no root and does not refresh repositories. Never test real package transactions
or downloaded scripts on the workstation; use disposable environments.

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release --locked -p pkgx-cli
```

Tests use fixtures, injected runners, temporary storage, harmless utilities and a
Ratatui TestBackend. Docker/Podman and APT are unavailable locally. Pacman 7.1.0 is checked
read-only on the host; Pacman transactions and cleanup ran only in a disposable
user-namespace Arch root, and recipe install/uninstall only in a loopback-only sandbox. Managed configuration
files are tested only against temporary HOME directories (no sandbox run yet).
APT (Debian bookworm, Ubuntu 24.04) and DNF5 (Fedora 44) transactions ran in rootless
disposable roots. Configured CI runs and macOS native validation remain pending. No host package
transaction, cleanup or installation script has run. See [validation status](docs/support.md) and [local results](docs/validation.md),
[manifest reference](docs/manifest.md), [recipe preparation](docs/recipes.md),
[failure/recovery](docs/recovery.md), [roadmap](pkgx.md) and [current scope](docs/continuation-plan.md).

In the TUI, Tab cycles through all loaded providers, including providers with an
empty inventory, then returns to the combined view. The text filter is preserved.
Switching providers clears any pending plan review; manifest previews still use
all providers loaded at startup.

Try the portable demo on Linux or macOS (no native package manager required):

```sh
cargo run -p pkgx-cli -- demo
```

This browser uses five synthetic packages and shows a DEMO banner. Try `/curl`,
Enter, `i`, Esc, Tab to change providers, and `q` to quit. Native plan previews,
installation and cleanup are unavailable in this mode. For a noninteractive view:

```sh
cargo run -p pkgx-cli -- demo --snapshot --json
```

The demo does not inspect installed packages, access the network or write a journal.

The TUI uses an atfq-inspired two-pane layout: providers and package counts on the
left, packages or the reviewed details on the right, rounded frames and ANSI
terminal colours. Use Left/Right (or h/l) to focus a pane, j/k to navigate,
Enter to open package information, and ? for all shortcuts. Tab still cycles
providers directly. Narrow terminals show only the content pane.

On wide terminals, an installed-package snapshot follows the selected row beside
the list. It uses the cached inventory; Enter still requests full native metadata.
Press p to hide or show this preview. Home/End jump to the first/last package,
PageUp/PageDown move by the visible page, and Shift-Tab selects the previous
provider. Detail pages stop scrolling at the last wrapped line. The package list
retains its viewport between redraws.

Inspired by omniscient's overview and data-confidence cues, the header summarizes
visible packages by explicit, dependency and unknown installation reason. Counts
follow the provider and search filter; loaded inventory is the denominator.
Yellow marks unknown intent rather than package health. The source badge
separates cached native inventory from synthetic demo data. ? opens a scrollable
help overlay; Esc or ? closes it without losing the current selection or review.

Experimental strict APT manifests use separate explicit management grants:
`pkgx manage apt:hello:amd64 --dry-run` previews a grant, `pkgx managed` lists them,
and `pkgx manage apt:hello:amd64 --forget` revokes one. No existing or newly installed
package is adopted automatically. See the [strict-mode workflow](docs/manifest.md)
and [build/install/archive instructions](docs/install.md).

`deps <provider:package>` (or d in the TUI) reports installed native expressions for
APT, Pacman, RPM and Homebrew formulae. Alternatives, constraints and virtual
capabilities are retained. These are metadata, not a resolved cross-provider graph.

For explicit privilege delegation on supported Debian/Ubuntu systems:

```sh
pkgx install apt:hello --sudo --dry-run
pkgx install apt:hello --sudo
pkgx apply -f system.toml --sudo
pkgx tui --provider apt --allow-apply --sudo
```

sudo owns password entry after transaction confirmation. The TUI briefly returns
to the normal terminal for that prompt. In noninteractive sessions, --yes still
requires a preauthorized sudo session or a passwordless policy; pkgx never reads a
password from stdin. Native revalidation follows authentication, and the eventual
transaction uses sudo -n. No-op/dry-run operations never authenticate. See the
[privilege policy](docs/adr/006-explicit-sudo.md) for fixed-path/configuration limits.
