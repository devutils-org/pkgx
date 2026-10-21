# Changelog

## Unreleased

- DNF4 (RHEL 8 and 9, Rocky, AlmaLinux): previews through DNF4's own resolver
  and confirmed, verified execution with `dnf`, as DNF5's (ADR 016). Validated
  in disposable Rocky 8 and 9 guests; Rocky 9 runs in CI.

## 0.1.0 — 2026-10-21

- Tagged releases: the artifacts workflow attests each archive's build provenance
  (Sigstore, `gh attestation verify`) and opens a draft GitHub release with
  `SHA256SUMS`. The macOS binary is not notarized.
- Run the Flatpak transaction checks in CI, in a disposable Arch container against a
  local repository.

- Add the apk (Alpine, apk-tools 2.12 and 3.x) and Portage (Gentoo, 3.0) providers:
  inventory with WORLD/world-set marks, native previews (`apk --simulate`, `emerge
  --pretend`) and confirmed, verified execution; Portage removal is `--depclean`
  of the reviewed version (ADR 014, ADR 015). Run them in Distroproof's Alpine
  and Gentoo inspection and transaction cells.
- Add an installed-release Distroproof matrix on 16 Linux images plus root and
  lockdown variants, with envprobe cross-checks and independent native inventories.
- Build GNU Linux archives against glibc 2.17 and verify the symbol floor; also
  verify the static musl archive. Add a manual portability workflow and a cell judge.
- Lower the APT implementation target from 2.6 to 1.6 after native inspection and
  transaction checks in disposable distro VMs (docs/distroproof.md).

- Run the managed-file checks in CI on Linux (bubblewrap) and macOS; the full CI
  matrix and the release-artifact workflow passed.
- Add experimental `files` manifest declarations for home-relative configuration
  files (ADR 013): strict validation, layering, confined source loading and
  `diff` states.
- Plan and write declared files with `plan -f`/`apply -f` (unified-diff previews,
  separate confirmation, atomic no-follow writes, receipts) and add
  `xpac file adopt|list|forget` with private backups (ADR 013).
- Add `xpac file remove` for unchanged managed files (inode-checked, backed up) and
  a bubblewrap sandbox for managed files (tests/sandbox/files-userns.sh).
- `xpac file list` reports intact/drifted/missing/unsupported receipts; the TUI
  manifest review shows declared file states read-only.
- Add experimental confirmed Pacman execution: --needed previews, HoldPkg/IgnorePkg
  and critical-package policy, repository pinning, version verification, journals,
  saved plans, TUI and exact-template sudo delegation (ADR 007).
- Apply mixed manifests as one reviewed transaction per executable provider.
- Fix export/diff of packages named like their provider (`pacman`, `apt`).
- Add a disposable user-namespace Arch root for native Pacman validation.
- Add tracked script install/uninstall with trust confirmation, home-relative
  ownership checks, private receipts and recorded uninstall actions (ADR 008).
- Add experimental DNF5 execution: stored-transaction previews, NEVRA-pinned
  execution with repository signature checks, verification and sudo (ADR 010).
- Add experimental Flatpak execution: single-remote ref/commit/runtime previews,
  --no-related installs, apps-only removal and origin/commit verification (ADR 011).
- Add experimental `recipe install --observe`: undeclared HOME changes are recorded
  as observed (never owned or removed) and classified `observed`.
- Add experimental Homebrew formula execution with dry-run previews, disabled
  autoremove and version verification; parse Homebrew 7 install reasons (ADR 012).
- Fix APT parsing of Debian packages named like providers (`apt`, `flatpak`, `dnf`)
  and RPM inventories containing imported `gpg-pubkey` headers.
- Add rootless debootstrap (Debian/Ubuntu) and Fedora roots for native validation.
- Classify audit findings as safe native cleanup, suggested cleanup or manual review;
  measure APT/Pacman caches; add confirmed `clean --cache` and reviewed
  `clean --orphans` (ADR 009). Pacman now advertises install/remove capabilities.

- Initialize Rust workspace, package identity and plan models.
- Add controlled subprocess runner and provider binary diagnostics.
- Add read-only APT inventory with architecture and native installation marks.
- Add JSON CLI output, fixture tests and Linux CI.

- Add APT search, control records, provenance and native transaction previews.
- Protect holds, Essential/Protected packages and additive installations.
- Add strict v1 TOML export/diff/plan/apply and reject unsupported schema features.
- Add experimental confirmed APT execution with stale-plan checks and verification.
- Bound subprocess output and pipe waits; kill process groups on Unix cancellation.
- Define Debian/Ubuntu container inspection CI (not yet run locally).

- Add Unix CLI interruption handling and private atomic transaction history.
- Add Pacman, RPM/DNF, scoped Flatpak and Homebrew consultation adapters.
- Add uncertain Pacman --print previews without a mutation executor.
- Add deterministic manifest profiles, inheritance and Linux/macOS package layers.
- Add mixed-provider preflight, diff and planning with explicit unsupported errors.
- Add native orphan audits and strictly read-only cleanup previews.
- Add a Ratatui browser with local filtering, provenance and plan review.
- Add pinned HTTPS/SHA-256 recipe preparation; downloaded code is never executed.
- Add readable terminal previews, atomic export and Bash/Zsh/Fish completions.
- Configure Arch/Fedora/macOS smoke CI and manual reviewable artifact builds.

- Review and verify APT manual-mark changes for already-installed dependencies.
- Reject ambiguous action architectures and detect effects outside reviewed actions.
- Inspect dpkg residual-configuration records without inferring removable files.
- Add strict saved package plans with bounded expiration and repeated stale checks.

- Add installed-only local search shared by the CLI and TUI for all adapters.
- Paginate transaction history and stream doctor journal statistics.
- Disable native purge/force-yes behavior and reject purge simulation records.
- Keep subprocess leaders unreaped until pipe closure to prevent PID/PGID reuse
  during bounded cleanup; cover inherited pipes with a compiled subprocess fixture.

- Add opt-in TUI APT execution with exact typed confirmation, shared native safety
  checks, journal records, worker progress and cancellation on terminal errors.

- Add runtime TUI provider selection with Tab, preserving local search and clearing pending reviews.

- Add a portable read-only `demo` browser and `demo --snapshot --json` using explicitly synthetic multi-provider data.

- Restyle the TUI with an atfq-inspired provider sidebar, rounded ANSI-colour panels, focus navigation, mode badges and contextual keyboard hints.

- Add a cached package preview on wide terminals, persistent list scrolling, page navigation and reverse provider cycling. Bound detail scrolling to the wrapped document and adapt shortcuts to the active pane.

- Add an omniscient-inspired filtered inventory summary, source/reason cues and a scrollable help overlay preserving the current review.

- Add installed native dependency reports (`deps`, TUI d) for APT, Pacman, RPM and
  Homebrew formulae, preserving constraints and virtual/alternative expressions.
- Add explicit APT management grants (`manage`, `managed`) and managed-only strict
  manifest diff/plan/apply, shared with the TUI. Lock registry writers and refuse
  stale grants, unmanaged cascades and removal of still-desired packages.
- Add deterministic versioned distribution archives with SHA-256, build metadata,
  license, docs and completions. Configure Linux host/musl and macOS artifact builds.

- Add explicit --sudo delegation for confirmed APT transactions in CLI and TUI.
  Authenticate before revalidation, preserve unprivileged inspection/journals and
  fail noninteractively on transaction-time credential expiry. Refuse alternate
  APT executables and native configuration/database overrides across the boundary.

- Give sudo a termination relay window before forced cleanup and bound child reaping so permission-denied or unresponsive subprocesses cannot block cleanup indefinitely.
