# Transactions, failure and recovery

APT execution is experimental until disposable native CI has passed. It requires
sufficient native privileges and explicit confirmation. --sudo explicitly
delegates only the native APT transaction after authentication and revalidation.
Inspect with --dry-run first. Never test real transactions on the workstation.

After confirmation, pkgx repeats inventory and native simulation. A changed plan
aborts. Requested install versions are pinned; native critical-package, held-package
and downgrade protections remain enabled. Install-only reconciliation forbids native
removals. APT's lock protects its transaction, not the preceding preview; this is not
an atomic repository snapshot, and dependency versions may still race.

Before invoking APT, the CLI persists intent to a private versioned JSON journal.
Default: `$XDG_STATE_HOME/pkgx/transactions`, or `$HOME/.local/state/pkgx/transactions`.
An explicit --state-dir selects the journal directory itself. New directories are
created privately; existing directories must already be private and owned by the
current user. pkgx does not chmod shared directories. Entries use restricted files,
staged writes, fsync and atomic replacement. Journal failure before execution aborts
the transaction. Failure to save the final result is reported, never silently ignored.

`pkgx history --json` reads the latest 20 records without creating state. Use
--limit (1–1000) and --offset to page older records. `doctor` computes counts one
record at a time rather than retaining every old plan in memory. `pkgx doctor --json`
reports pending records and journal-read errors. A record with no result is running,
interrupted or unknown; pkgx never converts it to success on startup. No automatic
rollback, repair or transaction replay exists. Entries contain native package details
but never command arguments, environment credentials or raw backend stderr.

SIGINT/SIGTERM cancels subprocess work on Unix; Ctrl-C in the TUI exits the browser.
Canceled/timed-out subprocess groups are terminated. A native transaction may be
partially applied; post-transaction verification can itself be unavailable after an
interrupt. Timeouts/output limits are protective bounds, not rollback guarantees.

After a failure, preserve the record, inspect `doctor`, `list`, `info` and native
package-manager diagnostics in an appropriate environment. Review any native repair
transaction independently. Do not delete lockfiles, uninstall undeclared packages,
replay scripts or assume that a matching inventory proves a transaction succeeded. Unplanned changes to the
untouched inventory are also reported as verification failures.
A nonzero native exit remains a failure even when planned identities are present.

Audit/clean only preview native orphan candidates. They never delete cache files,
residual configurations, home data, unknown paths or shared resources. Disk-space
estimates remain unknown; a native orphan mark is not sufficient removal authority.

Saved package plans use `plan apt:package --save preview.json --expires-in 300` and
`apply --plan preview.json`. The review lifetime cannot exceed one hour. Expired,
future-dated, malformed, foreign-provider or stale plans are refused. Confirmation
and fresh native checks still apply; --yes does not bypass expiration. See ADR 004.

Installing an already-installed dependency explicitly reviews its transition to a
native manual installation mark. Additive manifest apply still only requests missing
identities, preserving existing native dependency reasons on an idempotent no-op.

APT audit can report native config-files package records. These are not an inventory
of owned paths, and no purge, residual-file removal or cache deletion is supported.

The TUI is read-only by default. `tui --allow-apply` enables experimental APT execution:
review an install/removal or manifest plan, press x, type the exact word apply and
press Enter. A fresh native check still follows. The transaction runs on a worker
so Ctrl-C can cancel; terminal/event failure also requests cancellation before join.
After a failure, inspect history. Without --sudo, native privileges must already
be available. With --sudo, a missing/expired authorization fails without an
automatic retry or an additional password prompt during the transaction.

## Managed strict scope

Explicit grants are stored in `<state-dir>/managed/registry.json`, separately from
transaction records. `pkgx managed` reads them; `pkgx manage ID --forget` revokes a
grant without changing native packages. A recorded grant may outlive a package.
Changing the package's native version or install reason invalidates its removal
authority until a fresh grant is reviewed. State is never inferred from exports.

The `managed/lock` file serializes registry writers and strict execution. It is
removed on normal completion, including reported failures. A crash can leave it
behind: first inspect journal entries and active pkgx/native-manager processes.
Only after confirming no writer/transaction is active should the user manually
remove that exact registry lock file. pkgx never guesses from a PID or automatically
clears an interrupted lock. Do not delete the registry or native lock files as a
shortcut. Review a new native plan after any interrupted transaction.

Sudo authentication failures occur before native revalidation/journaling and start
no APT transaction. A later sudo -n failure is recorded like other execution
failures; inspect the result and current native state before retrying. The registry
and journal remain owned by the invoking user. In automation, obtain sudo credentials
through the system's own mechanism or configure the intended sudoers policy outside
pkgx; passwords are not passed through pkgx flags, JSON, logs or stdin.

For delegated transactions, cancellation first asks sudo to forward termination.
An unprivileged caller cannot guarantee that privileged/detached descendants have
stopped. If cleanup reports a process may still be running, inspect native processes
and locks before retrying. pkgx does not block forever waiting for an unkillable
child, and retains a background reaper while the CLI/TUI process remains alive.
