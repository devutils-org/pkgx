# ADR 002: APT planning and execution

Status: implemented experimentally; native container validation pending.

Inspection runs without escalation. Plans contain all native Inst, Remv and Conf
records, their version/repository detail and native architecture where present.
APT remains the dependency solver. An unqualified removal is resolved against
installed identities and refused when architecture would be ambiguous. Holds,
Essential and Protected packages are checked; missing protection metadata fails
closed. Install-only plans and executions use --no-remove.

Execution requires a reviewed preview and explicit confirmation (or --yes). The
executor independently repeats inventory and native simulation and compares the
whole plan immediately before calling APT. A canonical inventory comparison token
is not a cryptographic signature or a portable lockfile. No saved plan is trusted
as an executable command. Requested installation versions are pinned to reviewed
native versions. APT's own held/essential/downgrade protection remains enabled.

This is not an atomic snapshot: repository metadata, native configuration and
other package-manager processes may race between simulation and execution. Native
locks protect APT's transaction, not the earlier preview. Dependency versions can
still change during that interval; every planned action is checked against the
post-transaction inventory and discrepancies are reported as failures.

Without --sudo, the execution process must already have sufficient native
permissions. ADR 006 adds explicit sudo delegation after operation confirmation;
a failed transaction is never silently retried with elevation. Read-only commands
never need elevated privileges.

Execution captures status and verifies every planned operation against inventory,
including versions. Nonzero status remains failure even if inventory matches.
Interruptions and timeout failures may leave partial native changes; no rollback
is attempted or claimed. The CLI persists private versioned intent/result records; see docs/recovery.md.
The native transaction timeout is 30 minutes; inspection uses 30 seconds. On Unix,
cancellation terminates the subprocess group. CLI SIGINT/SIGTERM cancellation is connected through the session runner.

Native interface references:

- [Debian apt-get manual](https://manpages.debian.org/testing/apt/apt-get.8.en.html)
- [Debian apt-cache manual](https://manpages.debian.org/bookworm/apt/apt-cache.8.en.html)
