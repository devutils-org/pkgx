# M0 and first sprint: initial vertical slice (historical scope)

Scope: two crates, core models and typed errors, a mockable command runner, provider
binary detection, providers/doctor and explicit APT list commands, structured JSON.
The initial slice implemented no installation operations or script execution; later
work is described in docs/continuation-plan.md and the current README.

Assumptions: Linux for inventory; dpkg-query's installed status is authoritative.
APT marks are advisory and missing or conflicting marks produce unknown provenance.
At this initial stage other providers were detected without adapters; consultation
adapters have since been added (see docs/backends.md).

Validation: fixture inventory, multiarch identity, missing marks, malformed output,
missing tools, pre-cancellation, subprocess timeout, CLI version/error/JSON contracts.
CI runs fmt, clippy and tests without invoking a real package manager.

Continuation added a capability-based backend trait, mock inventory backend, CLI
help snapshot, native APT preview/execution and manifest planning. Remaining:
native-version certification and Debian/Ubuntu container evidence. Version parsing,
minimum-target gating and journal diagnostics have since been implemented.
M0 and M1 are not complete. No native integration has been run on the host.
