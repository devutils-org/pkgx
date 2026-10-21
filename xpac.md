# xpac — Agent Implementation Roadmap

> Status: implementation in progress (v0.1.0) | Target: Linux-first, macOS later | Language: Rust
>
> Purpose: This document is an actionable implementation plan for a coding agent. Execute milestones in order, keep changes small, test each backend in isolated environments, and never perform destructive host operations during development.

## 0. Product contract

Build **xpac**, a CLI-first, eventually TUI-based manager that unifies supported native package managers and tracked script installations. It must **not** replace native dependency solvers. Its distinctive features are provenance (`why`), declarative desired state (TOML), safe planning, audits, and conservative cleanup.

### Non-goals for v1

- Universal dependency solving across providers.
- Guaranteed rollback of native package-manager transactions.
- Automatically uninstalling arbitrary `curl | sh` installations.
- Deleting user data or unowned files based on heuristics.
- Full reproducible OS images (Nix/Guix replacement).
- System-wide configuration management (`/etc`, services). Home-relative user configuration files are planned separately in M8.

### Invariants

1. Read-only commands never modify host state.
2. All mutating commands support a preview/plan before execution; `apply` requires explicit confirmation unless `--yes` is supplied.
3. Never silently switch providers for a package.
4. Preserve backend-specific semantics; do not claim equivalent behavior where none exists.
5. Treat externally installed software and unknown files as **unmanaged** until proven otherwise.
6. No `curl | sh` piping; download, verify integrity, inspect metadata, then execute only with explicit authorization.
7. Do not execute scripts or mutate the host during automated tests. Use mocks, containers, and disposable VMs.
8. Plans are validated against the current state immediately before execution; abort or replan if stale.
9. Config manifests are data, not executable code; reject unexpected fields and untrusted command interpolation.
10. Logs redact secrets and never claim rollback succeeded without verification.

## 1. Technology and repository

- Rust stable, Cargo workspace.
- CLI: `clap`; serialization: `serde`, `toml`; errors: `thiserror`/`anyhow`; tracing: `tracing`.
- TUI later: `ratatui` + `crossterm`.
- Tests: Rust unit/integration tests, mock command runner, Linux container matrix, macOS CI when Brew support lands.
- Native backend integrations initially via carefully controlled subprocess adapters; do not write directly to package-manager databases.

Suggested structure:

```text
xpac/
  Cargo.toml
  crates/
    xpac-core/       # domain models, backend interfaces, errors
    xpac-backends/   # apt, pacman, dnf, flatpak, brew, scripts
    xpac-state/      # manifest parsing, inventory, diff, planner
    xpac-audit/      # provenance, orphans, cleanup findings
    xpac-cli/        # command-line frontend
    xpac-tui/        # interactive frontend (later)
  tests/fixtures/
  docs/adr/
  examples/
```

Start with the minimum crates needed; do not create empty abstraction layers just to match the layout.

## 2. Domain model and interfaces

Define:

- `ProviderId` (e.g. `apt`, `pacman`, `dnf`, `flatpak`, `brew`, `script`).
- `PackageId { provider, name, architecture_or_scope? }` (never identify solely by name).
- `InstalledPackage { id, version, install_reason, dependencies, metadata }`.
- `InstallReason`: explicit, dependency, unknown.
- `PackageProvenance`: provider, installation reason, dependents, origin, confidence.
- `ManagedResource`: declared files, links, services, configurations, with ownership and confidence.
- `DesiredState`, `ObservedState`, `Diff`, `Operation`, `Plan`, `ExecutionRecord`.
- `RiskLevel`: safe/native, requires_review, unknown/unsupported.

Use a **capability-based backend interface**: inventory, search, install, remove, dependency query, orphan query, dry-run, config inspection. Each backend advertises supported capabilities. Do not force unsupported features into fake implementations. A `CommandRunner` interface must make commands mockable, capture exit status/stdout/stderr, and enforce argument-based invocation (no implicit shell).

Planning and execution are separate: backends can propose native operations; the core assembles a plan; the executor validates and applies the plan. Where native tools cannot provide accurate dry-run data, clearly mark estimates as uncertain.

## 3. Milestones

### M0 — Foundation (1–2 weeks)

**Tasks**

- [x] Initialize workspace, formatting, linting, CI (`fmt`, `clippy`, `test`).
- [x] Define domain types, capability flags, typed errors, command-runner abstraction.
- [x] Add backend detection for OS, available binaries, and supported versions. (Minimum-target gating implemented; native certification pending.)
- [x] Define human-readable and `--json` output conventions.
- [x] Write ADRs for privilege escalation, package identity, and plan safety.
- [x] Add mock backend and golden fixtures for CLI tests.

**Acceptance**: `xpac --version`, `xpac providers`, `xpac doctor --json` run without elevated privileges; tests work on a machine without APT.

### M1 — APT CLI MVP (2–3 weeks)

**Tasks**

- [x] Implement APT/dpkg installed inventory, search, package info and version reporting.
- [x] Implement install/remove planning using native APT facilities; honor package holds and essential-package protections.
- [x] Implement `xpac list`, `search`, `info`, `install`, `remove`, `plan`, `why` (initial scope).
- [x] Expose native auto/manual installation reason and dependency information when available. (APT manual-mark transitions verified in rootless Debian/Ubuntu roots.)
- [x] Ensure explicit provider selection (`--provider apt`) and unambiguous defaults.
- [x] Integrate permission prompts only at execution time; never require root for inspection. (Explicit --sudo verified natively in Debian/Ubuntu roots, including the interactive PAM prompt through a pseudo-terminal; TUI suspension remains unit-tested only.)
- [x] Test against supported Debian/Ubuntu container images and negative/error cases. (Rootless Debian bookworm and Ubuntu 24.04 roots; CI containers configured.)

**Acceptance**: dry-run outputs a meaningful operation list; canceling leaves state unchanged; real install/remove works in a disposable test environment; `why` clearly labels unknown provenance.

### M2 — Declarative configuration (2–3 weeks)

**Tasks**

- [x] Version the TOML schema (`version = 1`), validate strictly, document migration policy.
- [x] Implement `xpac export`, `diff`, `plan -f`, `apply -f`.
- [x] Define additive vs strict synchronization; default to **additive** (never remove undeclared packages). (Strict mode is APT-grant scoped, ADR 005; verified natively in Debian/Ubuntu roots.)
- [x] Support platform/provider-specific sections and profile composition with deterministic precedence.
- [x] Normalize observed state and show drift with reasons and provider attribution.
- [x] Detect conflicts, stale plans, missing packages and unsupported operations before execution. (Fixture evidence plus native APT/Pacman/DNF CI transactions.)
- [x] Ensure repeated apply is a no-op when desired state already matches. (Verified natively for APT, Pacman and DNF.)

**Acceptance**: exported manifest can be parsed; apply is idempotent in integration tests; undeclared software is not removed by default; all destructive changes are previewed.

### M3 — Multi-provider support (3–4 weeks)

**Order**: Pacman → DNF → Flatpak → Homebrew (macOS).

**Tasks**

- [x] Implement each provider using the same capabilities contract. (APT, Pacman, DNF, Flatpak and Homebrew formulae; ADR 002, 007, 010–012.)
- [x] Record provider-specific package identity and install scope (system/user where relevant).
- [x] Handle ambiguous names and collisions with explicit provider qualification.
- [x] Test dependency/orphan semantics per backend, not via assumed universal behavior. (Native orphan removal verified for Pacman and Homebrew, APT audit natively; DNF/Flatpak orphan queries are unsupported by design.)
- [x] Add Linux distro CI and macOS Brew smoke tests.
- [x] Document unsupported provider features and limitations.

**Acceptance**: list/install/remove/plan work on supported environments; mixed-provider manifests reconcile deterministically; no accidental cross-provider removals.

### M4 — Tracked script installs (3–5 weeks)

**Tasks**

- [x] Introduce `script` provider with explicit installation recipes (source URL, pinned version, SHA-256, interpreter, install/uninstall steps, declared resources). (Exposed as `xpac recipe` commands, ADR 008.)
- [x] Download scripts to temporary storage, verify checksum, require trust confirmation before execution.
- [x] Store installation receipts and transaction metadata in a local registry.
- [x] Classify scripts as `tracked`, `observed` (experimental), or `unmanaged`. (Plus `tracked_incomplete`; ADR 008.)
- [x] Require explicit uninstall instructions; refuse automated removal if ownership is uncertain.
- [x] Add best-effort resource observation only behind an experimental flag; document platform and security limits. (`recipe install --observe`, HOME only; ADR 008.)
- [x] Reject scripts without integrity metadata by default, with a clearly marked manual override policy if needed. (No override exists.)

**Acceptance**: known recipe installs/uninstalls in a sandbox; tampered downloads fail; unknown scripts are never silently deleted; user sees what is and is not tracked.

### M5 — Audit and conservative cleanup (2–4 weeks)

**Tasks**

- [x] Implement `xpac audit`, `doctor`, `clean --dry-run`.
- [x] Detect native package-manager orphans and caches through native APIs/commands. (APT/Pacman caches; Brew/DNF/Flatpak caches unsupported.)
- [x] Identify residual configuration only where attribution is reliable. (dpkg package records only; paths/ownership not inferred.)
- [x] Present risk, provenance, estimated reclaimable space, and confidence for each finding.
- [x] Separate safe native cleanup, suggested cleanup, and advanced manual review. (ADR 009.)
- [x] Keep user home data, system-critical packages, unknown files, and shared resources protected by default.
- [x] Record before/after transaction results; report partial failures accurately.

**Acceptance**: default cleanup never deletes unmanaged paths; a user can inspect every proposed change; failed operations are surfaced, not hidden.

### M6 — Ratatui interface (3–4 weeks)

**Tasks**

- [x] Build package browser with local search/filter/provider selection. (Startup provider selection and runtime Tab cycling; repository search remains a CLI operation.)
- [x] Add package detail, dependency/provenance view, and orphan explorer.
- [x] Add manifest diff and plan review screens.
- [x] Add operation confirmation, progress, failure details, and keyboard help. (Experimental APT path; terminal/native validation pending.)
- [x] Reuse core services rather than duplicating business logic.

**Acceptance**: TUI supports the same core workflows as CLI, works in common terminal sizes, and does not execute changes without confirmation.

### M7 — Stabilization and v1.0 (3–5 weeks)

**Tasks**

- [x] Add regression suites for package locks, partial transactions, interruptions, stale plans, and provider failures.
- [x] Validate on a documented support matrix; run macOS smoke tests. (GitHub Actions run 39143547380 on 7b6d4a7: Debian bookworm/Ubuntu 24.04 APT, Arch Pacman and Fedora DNF5 transactions in disposable containers, bubblewrap recipe and managed-file sandboxes, macOS Homebrew transactions and managed files; see docs/support.md. Not a certification.)
- [x] Write installation, security, manifest, backend capability, and recovery documentation.
- [x] Provide shell completions, release binaries, changelog, reproducible build instructions where feasible. (artifacts run 39143553843: Linux gnu/musl and macOS arm64 archives with verified SHA-256 checksums, uploaded as reviewable workflow artifacts. v0.1.0 is published from artifacts run 39559531645 with build provenance attestations, verified after download; macOS notarization is out of scope.)
- [x] Perform security review of script execution, privilege boundaries, and cleanup operations. (Internal review recorded in docs/security-review.md; no independent audit.)

**Acceptance**: documented support matrix passes CI; all mutating operations are plan-driven; known limitations are published; release artifacts are verified.

### M8 — Managed user configuration files (planned, 3–5 weeks)

Goal: declare user configuration files (e.g. `~/.tmux.conf`) next to their packages and
reconcile them with the same diff → plan → apply review flow. This is file ownership,
not configuration semantics: xpac copies declared content, it does not understand,
merge or generate tmux/shell/editor options. Design: [ADR 013](docs/adr/013-managed-config-files.md).

Illustrative schema (proposed in ADR 013):

```toml
[packages.apt]
install = ["tmux"]

[files."~/.tmux.conf"]
source = "dotfiles/tmux.conf"   # relative to the manifest directory
mode = "0644"

[profiles.dev.files."~/.config/nvim/init.lua"]
source = "dotfiles/nvim/init.lua"
```

**Tasks**

- [x] ADR 013: schema (keyed by target, layered like packages, conflicts across layers are errors), versioning impact on `version = 1`, ownership/receipt model, copy vs symlink deployment. (docs/adr/013-managed-config-files.md.)
- [x] Parse and validate file declarations: home-relative targets only, no parent traversal, sources confined to the manifest directory (no symlink escape), size limit, literal content only (no templates, variables or interpolation). (Unit-tested.)
- [x] Observe target state without mutation: `missing`, `in_sync`, `drifted` (managed file changed locally since its receipt), `unmanaged_conflict` (existing file never written by xpac), plus non-regular targets. (Plus `outdated` and `undeclared`; fixture-tested.)
- [x] Integrate files into `diff`, `plan -f` and `apply -f`, after package transactions; previews show a content diff with an explicit warning that file contents are displayed. (CLI only; TUI refuses file manifests.)
- [x] Write atomically as the user (staging + rename), never follow a destination symlink, create parent directories with private permissions, refuse setuid/setgid modes; record a receipt (target, source digest, written digest, mode). (Temporary-HOME regressions; no sandbox run yet.)
- [x] Overwrite only files whose current digest matches their receipt; `drifted` and `unmanaged_conflict` targets are refused unless explicitly adopted (`xpac adopt-file`-style grant, previewed, the previous content kept as a private backup). (`xpac file adopt|list|forget`.)
- [x] Removing a declaration only forgets ownership; deleting a managed file requires an explicit, previewed prune limited to files still matching their receipt. Never delete unmanaged or drifted files. (`xpac file remove`, backed up first.)
- [x] Show managed files, drift and receipts in `audit`/TUI manifest review; `export` does not capture file contents. (`xpac file list` reports intact/drifted/missing/unsupported receipts; TUI manifest review shows file states read-only; provider `audit` is unchanged.)
- [x] Validate in a sandbox with a private HOME (loopback-only), including drift, conflicts, interrupted writes and repeated apply as a no-op. (tests/sandbox/files-userns.sh; interrupted writes not exercised.)

**Out of scope for M8**: system paths (`/etc`), services and reload actions (e.g. `tmux source-file` is suggested, never run), templating/per-host variables, secret management (contents are copied verbatim; storing secrets in manifests is discouraged), merging partial edits into foreign files.

**Acceptance**: a manifest with packages and a dotfile reconciles idempotently; existing unmanaged files are never overwritten without explicit adoption; local edits are reported as drift instead of being silently reverted; nothing is deleted without a matching receipt and confirmation.

## 4. Example manifest (illustrative schema)

```toml
version = 1

[system]
name = "dev-workstation"
mode = "additive"

[packages.apt]
install = ["git", "curl", "build-essential"]

[packages.brew]
install = ["neovim", "ripgrep", "fzf"]

[packages.flatpak]
install = ["org.mozilla.firefox"]

[[scripts]]
name = "example-tool"
version = "1.2.0"
url = "https://example.org/install.sh"
sha256 = "<verified-sha256-required>"
# Script recipes require explicitly modeled install/uninstall actions.
# The placeholder above is invalid until replaced with a verified digest.
```

Do not assume every section applies to every host: unsupported providers should be reported and handled according to a documented policy, not silently executed or ignored.

## 5. CLI target

```text
xpac providers
xpac list [--provider apt] [--json]
xpac search <query> [--provider apt]
xpac info <provider:package>
xpac why <provider:package>
xpac install <provider:package> [--dry-run]
xpac remove <provider:package> [--dry-run]
xpac export [-o system.toml]
xpac diff -f system.toml
xpac plan -f system.toml
xpac apply -f system.toml
xpac audit [--json]
xpac doctor
xpac clean --dry-run
xpac tui
```

Command syntax is a proposal; finalize and document it before implementing multiple frontends.

## 6. Agent execution protocol

For each milestone:

1. Read this roadmap and relevant ADRs; identify the smallest vertical slice.
2. Write or update a short milestone plan with scope, assumptions, and tests.
3. Implement a small coherent change, with unit tests and fixtures.
4. Run `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`.
5. Run provider integration tests only inside disposable environments, never on the user's workstation.
6. Update docs, changelog, and checklist; report exactly what passed, what failed, and what remains.
7. Stop and request a decision if a proposed feature could remove unmanaged files, bypass integrity checks, or require unsafe host-level operations.

**Definition of done for each milestone:** code + tests + docs + supported-platform evidence + explicit limitations. Do not mark tasks complete based only on compilation.

## 7. First sprint — executable backlog

- [x] Create Cargo workspace and `xpac-cli` binary.
- [x] Add `ProviderId`, `PackageId`, `InstalledPackage`, `Capability`, `Plan` types.
- [x] Add mockable `CommandRunner` with timeout and cancellation support.
- [x] Implement provider detection and `xpac providers`.
- [x] Implement read-only `xpac list --provider apt` via dpkg/APT metadata.
- [x] Add fixture-based parsing tests, CLI snapshot tests, and error handling.
- [x] Add CI for Linux build, fmt, clippy, tests.
- [x] Document safety invariants and supported behavior.

**First deliverable:** `xpac providers` and `xpac list --provider apt` reliably report local information without root privileges or system changes.

## 8. Open design decisions (record as ADRs)

- Naming and CLI grammar (`xpac` is provisional).
- State registry storage (SQLite vs structured files), schema versioning and migration.
- Privilege escalation model (`sudo`, `polkit`, native tooling) and threat boundaries.
- Manifest profile inheritance and host-specific overrides.
- Locking/concurrency with native package managers.
- Script receipt format, resource ownership, and secure execution model.
- Plan serialization, expiration, and recovery after partial execution.
- Supported OS/distribution and package-manager version matrix.
- Managed configuration files (M8): proposed in ADR 013 (copy deployment, explicit adoption with backups, `version = 1` extension); templating/secrets remain open.

**Default recommendation:** Linux-first; APT first; declarative additive manifests; explicit verified script recipes; macOS/Brew after core semantics are stable; TUI after CLI workflows are proven.

## Implementation progress

M0–M8 tasks are complete; native acceptance evidence comes from disposable roots and CI
(docs/support.md), not from a certification.
Implemented and fixture-tested: core identities/models/capabilities, subprocess
bounds/cancellation, provider diagnostics, APT inspection/planning/experimental
execution, explicit manual-mark actions, bounded saved-plan review/expiration,
private atomic transaction journals, additive manifests and explicit managed-only strict APT scope,
profile/platform composition, mixed-provider preflight and provider-aware diff.

M3 has consultation adapters for Pacman, RPM/DNF, Flatpak and Homebrew. Pacman
preview is an uncertain native --print estimate; Pacman has an experimental
executor (ADR 007) validated in a disposable user-namespace Arch root.
M4 has strict recipe check, HTTPS/SHA-256 preparation and confirmed home-scoped
install/uninstall with receipts (ADR 008), validated in a loopback-only sandbox;
undeclared HOME effects can be observed experimentally (--observe). M5 classifies safe native cleanup (measured
APT/Pacman caches), suggested cleanup (orphans) and manual review (residual records);
confirmed native cache cleanup and reviewed orphan removal exist (ADR 009);
residual-configuration purge remains unsupported. M6 has
a Ratatui browser with details/provenance/orphans/plans, read-only by default,
with opt-in typed confirmation for any executable provider and cancellable worker
execution. M7 has expanded
regressions, recovery/security documentation, completion definitions and a manual
artifact-building workflow whose Linux/macOS archives were built and checksum-verified
in CI; no release has been published or certified. M8 manages home-relative
configuration files (ADR 013).

Local checks use fixtures, injected runners, temporary files and TestBackend.
Native transactions, recipe scripts and cleanup ran only inside disposable rootless
roots (tests/sandbox/); no host package transaction, downloaded script or cleanup
operation has run on the workstation. The configured CI jobs (containers, recipe and
managed-file sandboxes, macOS Homebrew and managed files) passed in GitHub Actions run 39143547380.
Flatpak transactions run in CI too (run 39558744977). Tagged releases attest build
provenance and open a draft release; macOS notarization stays out of scope. See
docs/support.md for limits.

Latest local checkpoint (2026-10-18): 169 Rust tests and 2 packaging tests passed;
native scenarios passed in rootless disposable roots for Debian bookworm, Ubuntu
24.04, Arch, Fedora 44, Flatpak and Homebrew on Linux; see docs/validation.md.
