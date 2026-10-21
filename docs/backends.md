# Backend capabilities and identity

Implementation, local native validation and hosted CI evidence are distinct.
The [Distroproof matrix](distroproof.md) records the measured Linux coverage.
CLI adapter creation requires a recognizable native version at or above these implementation targets:
APT 1.6, Pacman 6.0, DNF 4.0 (including DNF5 for RPM consultation), Flatpak 1.14,
Homebrew 4.0, apk-tools 2.12 (3.x included), Portage 3.0. Older or unrecognized versions are refused. These are policy targets,
not claims that every newer release has been tested. Provider diagnostics expose
the parsed version, minimum-target check and `certified: false`.

| Provider | Inventory | Info / why | Repository search | Plan | Execute | Orphan preview |
| --- | --- | --- | --- | --- | --- | --- |
| APT | dpkg + APT marks | Native records / reverse dependencies | POSIX regex, cached metadata | Native simulation | Experimental, confirmed | Native simulation with hold/critical checks |
| Pacman | C-locale `-Qi` records | Installed records / native reason | POSIX regex, existing sync database | `--print` estimate + HoldPkg/IgnorePkg policy | Experimental, confirmed (ADR 007) | `-Qdtq` with native dependency marks |
| DNF | RPM installed database | Installed records / reason unknown | Unsupported | dnf5 `--store` resolved transaction; DNF4's own resolver through its Python module | Experimental, confirmed (ADR 010, DNF4 ADR 016) | Unsupported |
| Flatpak | Default user and system installations | Installed records / reason unknown | Unsupported | Single-remote ref/commit/runtime preview (uncertain) | Experimental, confirmed, apps-only removal (ADR 011) | Unsupported |
| Homebrew | JSON v2, installed kegs/casks | Installed records / receipt reason when available | Unsupported | Formula `--dry-run` (uncertain) | Experimental, confirmed, formulae only (ADR 012) | Dependency leaves with native mark checks |
| apk | `apk list --installed` + WORLD constraints | Installed records / WORLD mark | Unsupported | `--simulate` against local indexes (uncertain) | Experimental, confirmed, versions verified after (ADR 014) | Unsupported |
| Portage | Installed database (`/var/db/pkg`) + world set | Installed records / world selection | Unsupported | `emerge --pretend` (uncertain); removal `--depclean` | Experimental, confirmed, pinned `=cat/name-version` (ADR 015) | Unsupported |

Examples of fully qualified identities:

```text
apt:curl:amd64
pacman:curl:x86_64
dnf:curl:x86_64
flatpak:app/org.example.App/x86_64/stable:user
flatpak:runtime/org.example.Platform/x86_64/stable:system
brew:curl:formula
brew:homebrew/cask/firefox:cask
apk:strace:x86_64
portage:dev-lang/python:3.12
```

APT, Pacman, RPM and apk permit scope-free lookup when installed matching is unambiguous;
a Portage identity's scope is its slot, needed only when several slots are installed.
Flatpak requires kind, full ID, architecture, branch and scope. Named custom system
Flatpak installations are not queried; only default user/system are covered. Localized
or malformed native output fails instead of returning a misleading empty inventory.
Flatpak's native active-commit prefix is preserved; when the app-data version is absent,
that abbreviated commit is reported with an explicit version-source metadata field.
It is not a complete commit digest or reproducibility guarantee. `--all` includes runtime extensions.

Homebrew formula/cask identity is explicit. Multiple installed kegs and RPM
installonly versions remain separate observed entries. A name/scope-only manifest
requires their presence without pinning versions. `why` refuses ambiguous multiple
versions because it cannot identify a unique installation event.

DNF uses read-only RPM queries instead of running DNF with possible cache/log writes.
RPM metadata alone does not establish DNF installation reason or orphan status.
Homebrew queries disable automatic updates, analytics and install cleanup. No
provider refreshes repositories during inspection. No adapter edits native databases.

Pacman `--print` lists targets/dependencies, not the complete transaction including
replacements, conflicts and hooks. Its plans remain uncertain; the experimental
confirmed executor revalidates the preview, pins the repository and verifies
installed versions afterwards (ADR 007). A missing capability returns an explicit unsupported error;
providers are never substituted.

Primary native references:

- [Pacman manual](https://man.archlinux.org/man/pacman.8.en)
- [DNF command reference](https://dnf.readthedocs.io/en/stable/command_ref.html)
- [Flatpak command reference](https://docs.flatpak.org/en/latest/flatpak-command-reference.html)
- [Homebrew JSON interface](https://docs.brew.sh/Querying-Brew)

APT audit additionally reports dpkg config-files records as review-only findings.
Concrete architecture is required for every executable native action. APT plans
include explicit manual-mark transitions; native execution verifies untouched
inventory as well as the intended actions.

`search --installed --provider ...` performs literal case-insensitive filtering over
a single installed-inventory snapshot for every consultation adapter. It requires
neither repository search nor a solver and never refreshes repositories. The TUI
uses the same filtering service. Default search keeps its native-provider semantics.

`deps` exposes installed dependency expressions for APT (dpkg control fields),
Pacman (native dependency/optional/reverse relationship fields), RPM (`--requires`)
and unambiguous Homebrew formulae. It rejects absent or ambiguous installed
versions. RPM requirements include rich expressions, file and virtual capabilities.
Homebrew formula metadata may differ from an installed keg's historical dependency
versions; cask dependency queries remain unsupported. Flatpak dependency queries
remain unsupported. No report fabricates resolved edges from names or alternatives.
