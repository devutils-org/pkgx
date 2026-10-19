# ADR 006: Explicit APT sudo delegation

Status: implemented; verified in rootless Debian/Ubuntu roots (noninteractive policy
and an interactive PAM prompt through a pseudo-terminal). Pacman and DNF reuse the
same delegation with their own exact templates (ADR 007, ADR 010).

Privilege elevation is opt-in with --sudo. The flag is accepted only for install,
remove, apply, or tui --allow-apply. Operation review/confirmation still comes first;
--yes approves a transaction, not a password prompt. Dry runs and no-op manifests
never authenticate. Inspection has no elevation path.

After confirmation, sudo validates credentials before native revalidation. With
interactive stdin/stderr, xpac runs /usr/bin/sudo -v -u root in the normal foreground
terminal, with a 120-second timeout and signal cancellation. Only sudo reads the
password via its terminal UI. xpac does not read, pipe, store or log credentials.
stdout is closed for the credential process so JSON remains a single result.
Without interactive stdin/stderr, sudo -n -v -u root requires an existing authorization or
passwordless policy and refuses immediately otherwise. Authentication failure
prevents native execution and creates no transaction intent.

The TUI temporarily leaves raw mode/alternate screen for authentication and resumes
it on success or refusal. No raw-mode password entry is attempted. The subprocess
wrapper then delegates only an executor-marked APT transaction using fixed argv:
/usr/bin/sudo -n -u root -- /usr/bin/apt-get <generated native args>. Credential expiry or
sudo policy refusal during execution is a recorded failure, never a fresh prompt.

The system /usr/bin/apt-get must be the executable selected by the trusted PATH.
APT_CONFIG, DPKG_ROOT and DPKG_ADMINDIR overrides are refused rather than silently
using different databases/configuration across the privilege boundary. sudo policy,
its fixed system binary, native package configuration and repositories are trusted.
No arbitrary command, environment override, plan-supplied executable, inspection,
manifest parsing, journal write or management grant is elevated.

Saved-plan expiry, typed native-plan/baseline comparison, management-registry locks,
holds/critical-package checks and post-transaction verification remain mandatory.
Revalidation after authentication accounts for state changes while a password was
being entered. APT still owns its transaction lock; this is not an atomic lock
covering the earlier simulation and no rollback is promised.

Tests use injected authenticators/runners and harmless sleep processes. The native
CI script configures an isolated non-root user and sudoers rule inside disposable
Debian/Ubuntu containers only; it tests refused credentials, unprivileged previews,
confirmed installation/removal and user-owned private journals. That script has not
run locally. PAM behavior, interactive sudo and TUI suspension still need native
terminal validation. No sudo command was executed on the development host.

Native option/policy references: [sudo(8)](https://manpages.debian.org/bookworm/sudo/sudo.8.en.html)
and [sudoers(5)](https://manpages.debian.org/bookworm/sudo/sudoers.5.en.html).
The noninteractive option refuses password prompts, and passwordless validation
still depends on sudoers policy; xpac never changes that policy.

Cancellation sends SIGTERM to the delegate's process group, preserving the
unreaped leader while sudo has up to three seconds to relay it. Forced cleanup
follows, with at most 200 ms of synchronous reaping. An unexited child retains a
background reaper and produces a cleanup failure saying it may still be running;
a permission-denied kill must never lead to an unbounded wait. Privileged or
detached descendants cannot be guaranteed stopped by the unprivileged caller.
Their state remains unknown after interruption; no rollback or successful native
cancellation is inferred. Harmless subprocess tests cover the bounded cleanup
path, and the inherited-pipe process-group regression remains enabled.
