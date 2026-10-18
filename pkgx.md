# pkgx — Agent Implementation Roadmap

> Status: implementation in progress (v0.1.0) | Target: Linux-first, macOS later | Language: Rust
>
> Purpose: This document is an actionable implementation plan for a coding agent. Execute milestones in order, keep changes small, test each backend in isolated environments, and never perform destructive host operations during development.

## 0. Product contract

Build **pkgx**, a CLI-first, eventually TUI-based manager that unifies supported native package managers and tracked script installations. It must **not** replace native dependency solvers. Its distinctive features are provenance (`why`), declarative desired state (TOML), safe planning, audits, and conservative cleanup.

### Non-goals for v1

- Universal dependency solving across providers.
- Guaranteed rollback of native package-manager transactions.
- Automatically uninstalling arbitrary `curl | sh` installations.
- Deleting user data or unowned files based on heuristics.
- Full reproducible OS images (Nix/Guix replacement).

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
pkgx/
  Cargo.toml
  crates/
    pkgx-core/       # domain models, backend interfaces, errors
    pkgx-backends/   # apt, pacman, dnf, flatpak, brew, scripts
    pkgx-state/      # manifest parsing, inventory, diff, planner
    pkgx-audit/      # provenance, orphans, cleanup findings
    pkgx-cli/        # command-line frontend
    pkgx-tui/        # interactive frontend (later)
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

**Acceptance**: `pkgx --version`, `pkgx providers`, `pkgx doctor --json` run without elevated privileges; tests work on a machine without APT.

### M1 — APT CLI MVP (2–3 weeks)

**Tasks**

- [x] Implement APT/dpkg installed inventory, search, package info and version reporting.
- [x] Implement install/remove planning using native APT facilities; honor package holds and essential-package protections.
- [x] Implement `pkgx list`, `search`, `info`, `install`, `remove`, `plan`, `why` (initial scope).
- [ ] Expose native auto/manual installation reason and dependency information when available. (Implemented and fixture-tested; APT native acceptance pending.)
- [x] Ensure explicit provider selection (`--provider apt`) and unambiguous defaults.
- [ ] Integrate permission prompts only at execution time; never require root for inspection. (Explicit --sudo CLI/TUI path implemented with mocked refusal/cancellation/order checks; native PAM acceptance pending.)
- [ ] Test against supported Debian/Ubuntu container images and negative/error cases.

**Acceptance**: dry-run outputs a meaningful operation list; canceling leaves state unchanged; real install/remove works in a disposable test environment; `why` clearly labels unknown provenance.

### M2 — Declarative configuration (2–3 weeks)

**Tasks**

- [x] Version the TOML schema (`version = 1`), validate strictly, document migration policy.
- [x] Implement `pkgx export`, `diff`, `plan -f`, `apply -f`.
- [ ] Define additive vs strict synchronization; default to **additive** (never remove undeclared packages). (Explicit APT management grants, registry locking, strict diff/plan/apply and TUI implemented; native acceptance pending.)
- [x] Support platform/provider-specific sections and profile composition with deterministic precedence.
- [x] Normalize observed state and show drift with reasons and provider attribution.
- [x] Detect conflicts, stale plans, missing packages and unsupported operations before execution. (Fixture/simulated evidence; native CI pending.)
- [ ] Ensure repeated apply is a no-op when desired state already matches.

**Acceptance**: exported manifest can be parsed; apply is idempotent in integration tests; undeclared software is not removed by default; all destructive changes are previewed.

### M3 — Multi-provider support (3–4 weeks)

**Order**: Pacman → DNF → Flatpak → Homebrew (macOS).

**Tasks**

- [ ] Implement each provider using the same capabilities contract.
- [x] Record provider-specific package identity and install scope (system/user where relevant).
- [x] Handle ambiguous names and collisions with explicit provider qualification.
- [ ] Test dependency/orphan semantics per backend, not via assumed universal behavior.
- [x] Add Linux distro CI and macOS Brew smoke tests.
- [x] Document unsupported provider features and limitations.

**Acceptance**: list/install/remove/plan work on supported environments; mixed-provider manifests reconcile deterministically; no accidental cross-provider removals.

### M4 — Tracked script installs (3–5 weeks)

**Tasks**

- [x] Introduce `script` provider with explicit installation recipes (source URL, pinned version, SHA-256, interpreter, install/uninstall steps, declared resources). (Exposed as `pkgx recipe` commands, ADR 008.)
- [x] Download scripts to temporary storage, verify checksum, require trust confirmation before execution.
- [x] Store installation receipts and transaction metadata in a local registry.
- [ ] Classify scripts as `tracked`, `observed` (experimental), or `unmanaged`. (tracked/tracked_incomplete/unmanaged implemented; `observed` pending with resource observation.)
- [x] Require explicit uninstall instructions; refuse automated removal if ownership is uncertain.
- [ ] Add best-effort resource observation only behind an experimental flag; document platform and security limits.
- [x] Reject scripts without integrity metadata by default, with a clearly marked manual override policy if needed. (No override exists.)

**Acceptance**: known recipe installs/uninstalls in a sandbox; tampered downloads fail; unknown scripts are never silently deleted; user sees what is and is not tracked.

### M5 — Audit and conservative cleanup (2–4 weeks)

**Tasks**

- [x] Implement `pkgx audit`, `doctor`, `clean --dry-run`.
- [ ] Detect native package-manager orphans and caches through native APIs/commands.
- [x] Identify residual configuration only where attribution is reliable. (dpkg package records only; paths/ownership not inferred.)
- [x] Present risk, provenance, estimated reclaimable space, and confidence for each finding.
- [ ] Separate safe native cleanup, suggested cleanup, and advanced manual review.
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
- [ ] Validate on a documented support matrix; run macOS smoke tests.
- [x] Write installation, security, manifest, backend capability, and recovery documentation.
- [ ] Provide shell completions, release binaries, changelog, reproducible build instructions where feasible. (Local versioned archives/checksums and deterministic packaging implemented; multi-platform release execution pending.)
- [ ] Perform security review of script execution, privilege boundaries, and cleanup operations.

**Acceptance**: documented support matrix passes CI; all mutating operations are plan-driven; known limitations are published; release artifacts are verified.

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
pkgx providers
pkgx list [--provider apt] [--json]
pkgx search <query> [--provider apt]
pkgx info <provider:package>
pkgx why <provider:package>
pkgx install <provider:package> [--dry-run]
pkgx remove <provider:package> [--dry-run]
pkgx export [-o system.toml]
pkgx diff -f system.toml
pkgx plan -f system.toml
pkgx apply -f system.toml
pkgx audit [--json]
pkgx doctor
pkgx clean --dry-run
pkgx tui
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

- [x] Create Cargo workspace and `pkgx-cli` binary.
- [x] Add `ProviderId`, `PackageId`, `InstalledPackage`, `Capability`, `Plan` types.
- [x] Add mockable `CommandRunner` with timeout and cancellation support.
- [x] Implement provider detection and `pkgx providers`.
- [x] Implement read-only `pkgx list --provider apt` via dpkg/APT metadata.
- [x] Add fixture-based parsing tests, CLI snapshot tests, and error handling.
- [x] Add CI for Linux build, fmt, clippy, tests.
- [x] Document safety invariants and supported behavior.

**First deliverable:** `pkgx providers` and `pkgx list --provider apt` reliably report local information without root privileges or system changes.

## 8. Open design decisions (record as ADRs)

- Naming and CLI grammar (`pkgx` is provisional).
- State registry storage (SQLite vs structured files), schema versioning and migration.
- Privilege escalation model (`sudo`, `polkit`, native tooling) and threat boundaries.
- Manifest profile inheritance and host-specific overrides.
- Locking/concurrency with native package managers.
- Script receipt format, resource ownership, and secure execution model.
- Plan serialization, expiration, and recovery after partial execution.
- Supported OS/distribution and package-manager version matrix.

**Default recommendation:** Linux-first; APT first; declarative additive manifests; explicit verified script recipes; macOS/Brew after core semantics are stable; TUI after CLI workflows are proven.

## Implementation progress

M0–M2 remain in progress; their native acceptance evidence is not yet available.
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
experimental observation of undeclared effects is not implemented. M5 has native orphan audit previews
and read-only clean, plus native residual-configuration record inspection;
caches/residual-config removal remains unsupported. M6 has
a Ratatui browser with details/provenance/orphans/plans, read-only by default,
with opt-in typed APT confirmation and cancellable worker execution. M7 has expanded
regressions, recovery/security documentation, completion definitions and a manual
artifact-building workflow; no release has been published or certified.

Local checks use fixtures, injected runners, temporary files, TestBackend and
read-only native Pacman consultation.
No host package transaction, downloaded script or cleanup operation has run.
Docker/Podman and APT are absent here. Pacman 7.1.0 read-only inventory/dependency/provenance checks passed for a 301-package inventory. Configured CI
for Debian/Ubuntu transactions, Arch/Fedora consultation and macOS smoke has not
run locally and is not counted as passed. See docs/support.md for limits.

Latest local checkpoint: 134 Rust tests and 2 packaging tests passed; see docs/validation.md.
