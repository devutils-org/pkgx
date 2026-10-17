# Autonomous continuation

Advance every independently testable slice without running host transactions:

1. Connect CLI interrupts to controlled subprocess cancellation; persist transaction
   outcomes through a versioned local journal with restricted atomic writes.
2. Extend validated identity and read-only inventory/search/info adapters in roadmap
   order: Pacman, DNF, Flatpak, Homebrew. Keep unavailable simulation/execution
   capabilities unsupported rather than invent dependency-solver behavior.
3. Add deterministic manifest profiles/platform resolution and provider-aware diff.
4. Add evidence-backed audit/orphan previews and a read-only TUI package browser.
5. Expand regression tests, support/recovery docs and disposable-platform CI.

Use mocks for backend commands and temporary directories for registry tests. Never
invoke a native package transaction or installation script on the workstation.
Native and terminal-interaction validation remain explicitly pending when unavailable.

Additional implemented slices: expiring saved package plans, explicit native manual
marks, verification of untouched inventory, residual dpkg records, installed-only
search, paginated history and streamed diagnostics. The TUI now has an explicit
--allow-apply gate for typed, reviewed APT execution with worker cancellation. These
changes have simulated/unit evidence; native certification is still pending.

Installed dependency reports, deterministic archive packaging, and explicit APT
management grants/strict synchronization are now implemented and regression-tested.
See completion-plan.md and ADR 005 for acceptance boundaries.

Explicit APT --sudo delegation now authenticates only after confirmation and
before native revalidation, with no elevated inspection/state writes. ADR 006
records the boundary and pending disposable PAM/terminal acceptance evidence.
