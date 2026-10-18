# Changelog

## 0.1.0 — unreleased

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
