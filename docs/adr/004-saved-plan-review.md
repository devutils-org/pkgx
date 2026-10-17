# ADR 004: Saved plans are review data, not executable commands

Status: implemented with simulated execution tests; native validation pending.

`plan provider:package --save file.json` writes a strict v1 JSON review bundle with
provider, literal package requests, native preview, full observed baseline, creation
and expiration times. Default lifetime is 300 seconds, allowed range 1–3600 seconds.
Manifest/multi-provider bundles cannot be saved yet. Writes use the atomic export
writer; symlink destinations and oversized files are refused.

`apply --plan file.json` validates structure, identities, provider consistency,
baseline token, schema version and time window. It presents the stored plan and
requires the usual explicit confirmation. The executor repeats native inventory and
simulation after confirmation and compares the full typed plan. Edited/old snapshots
are not silently refreshed or trusted. Versions are reconstructed from reviewed native
actions and validated identities; no serialized shell command is ever executed.

Expiration is checked on load, after confirmation, after native revalidation and
immediately before execution following durable journal creation. If it expires after
journal creation, the result records that the native transaction was not started.
A clock value outside the recorded review window invalidates the plan. Expiration
controls transaction start; a transaction already started may finish after the deadline. No cryptographic
signature is claimed; users must review files they choose, especially with --yes.

`apply --plan ... --dry-run` displays stored review data and explicitly labels that
native state was not revalidated. It supports inspecting Pacman estimates, but only
APT plans can execute. Saved records never bypass critical-package or hold protections.

Plans include concrete architectures and a baseline inventory. Already-installed
APT dependency requests include a MarkExplicit action, so manual-reason changes are
reviewed instead of skipped as a false no-op. Post-execution verification checks all
reviewed actions plus the untouched inventory; unexpected additions/removals/version
or reason changes produce a failed record. This detects races but cannot undo native
side effects or guarantee atomicity across the native solver/transaction boundary.
