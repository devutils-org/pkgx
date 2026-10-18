# Implementation security review — development snapshot

This is a source and regression-test review, not native-platform certification or an
independent security audit. Native execution remains experimental.

Implemented controls and regression evidence:

- Native invocations use argv, validated provider-scoped identities and fixed programs;
  no implicit shell or manifest command interpolation exists.
- APT preview/execution disables purge and force-yes and retains held/critical-package
  protection. Purge records fail rather than disappearing from an operation list.
- Manual-reason transitions are explicit actions. Every executable action needs one
  concrete architecture. Native version/architecture ambiguity fails closed.
- Full typed baseline comparison follows user confirmation. After execution, planned
  actions and untouched inventory are checked. Native errors never become success
  merely because some desired packages are present.
- Saved plans are strict bounded JSON data with provider/identity/baseline checks and
  a bounded review lifetime. Expiration after journaling prevents native start and
  records the canceled intent. Serialized commands are not accepted.
- Subprocess reads are bounded and concurrent. The leader remains unreaped until pipe
  closure so cleanup cannot use a recycled PID/PGID while descendants retain pipes.
  Timeout/cancellation tests use harmless processes, including an inherited-pipe fixture.
  Sudo delegation receives a catchable termination signal before forced cleanup.
  Cleanup retains the leader until the final group signal, bounds synchronous
  reaping and reports an unexited child instead of waiting indefinitely. Privileged
  descendants can remain active; interruption never implies rollback.
- Transaction intent is durable before native invocation. Atomic restricted files and
  private owned directories are required. Shared directories are rejected, not chmodded.
  Corrupt journals fail diagnostics rather than being silently called healthy.
- Recipe preparation restricts HTTPS, credentials, redirects, size and time; curlrc is
  disabled. Bounded binary capture enforces size even when native curl's content-length
  handling differs. Hash mismatch discards staging. No interpreter is invoked.
- Recipe execution (ADR 008) needs trust confirmation, a non-root user, an explicit
  uninstall action and home-relative declared resources that are absent beforehand.
  The stored artifact is re-hashed before each run and passed by path to a fixed
  interpreter. Receipts claim only resources created by the install; uninstall is
  refused when owned files changed and pkgx itself never deletes resource paths.
  Scripts are not confined: undeclared side effects run with user permissions.
- Audit reads only native cache directories reported by the package manager and
  never traverses arbitrary data paths or purges configurations. Cleanup runs only
  the exact native cache commands or the reviewed removal executor (ADR 009).
- TUI mutation is off by default. Enabling it still requires a reviewed nonempty APT
  plan and exact typed confirmation. A worker reuses core revalidation/journaling.
  Terminal/event errors request cancellation before a worker is joined.
- Strict APT scope is delegated by separate confirmed management grants, never
  inferred from native marks. All native removals must remain in the reviewed
  undeclared grant set; cascades into unmanaged or still-desired identities fail.
  Registry writer locking and repeated snapshot checks protect against pkgx writers
  and detect state edits during revalidation. Shared paths and symlinks fail closed.

- Privilege delegation requires --sudo and operation confirmation, followed by
  authentication and fresh native validation. The fixed system sudo/apt-get paths
  and matching inspection executable are required. Native database/config overrides
  fail. Only marked APT execution is wrapped; no inspection/state write is elevated.
  Password entry belongs to sudo in the normal terminal and is never captured by
  pkgx. Transaction-time sudo is noninteractive. Injected tests cover refusal,
  cancellation, credential expiry and inspection/transaction separation.

Remaining boundaries and evidence gaps:

Native PATH, binaries, repositories and package-manager configuration remain trusted.
No cross-provider atomic transaction, native solver replacement, rollback guarantee
or script confinement exists. Explicit APT sudo delegation is implemented;
its native PAM/policy/terminal acceptance remains pending. APT's own lock
covers its transaction, not the earlier simulation; races can be detected afterward,
not prevented atomically. Native command cancellation can leave partial changes.

The Debian/Ubuntu/Arch/Fedora/macOS CI jobs passed (GitHub Actions run 39143547380). TUI widget,
confirmation and worker-cancellation logic is tested. Linux pseudo-terminal demo
smoke tests and native read-only Pacman checks passed; macOS terminal validation of
the TUI remains pending. Cross-provider strict execution, system-scope
recipes and observation of undeclared script effects stay unsupported.

## Internal review of new boundaries — 2026-10-18

Scope: the Pacman, DNF, Flatpak and Homebrew executors (ADR 007, 010–012), script
execution and receipts (ADR 008), cleanup execution (ADR 009) and sudo delegation
for the new providers. This is an internal source review backed by injected-runner
regressions and the disposable-root runs in validation.md, not an independent audit.

Confirmed properties:

- Every executor rebuilds native arguments from validated identities and the fresh
  preview; no saved or deserialized command is executed. Identity tokens start with
  an alphanumeric byte, so no target can be read as an option; DNF targets must be
  valid NEVRAs and Flatpak remotes/refs are validated tokens.
- sudo delegation is an allow-list of exact argument prefixes (APT install, Pacman
  `--sync --needed`/`--remove`, DNF install/remove, the two cache commands) plus
  validated targets, to fixed system binaries that must match PATH; environment
  overrides are refused. Flatpak and Homebrew are never run through sudo, and
  Homebrew is refused as root.
- Options that would widen a reviewed transaction are never produced: no database
  refresh or system upgrade (Pacman), no autoremove (DNF `--no-autoremove`,
  `HOMEBREW_NO_AUTOREMOVE`), no related refs (Flatpak `--no-related`), no
  force/ignore-dependency/cascade/nosave options. Native verification caught two
  defaults that would otherwise have widened transactions (Homebrew autoremove,
  replay skipping OpenPGP checks), and both are now excluded by design.
- Every executor verifies planned versions/commits and the untouched inventory and
  reports differences as failures without claiming rollback.
- Recipe scripts run only as a non-root user after trust confirmation, through a
  fixed interpreter, from a private store re-hashed before each run. Receipts claim
  only resources that were absent and then created; pkgx never deletes resource
  paths. Observation is metadata-only and never authorizes removal.
- Cache cleanup measures only native cache directories and runs only the native
  command; orphan cleanup goes through the reviewed removal executor.

Residual risks:

- Recipe scripts inherit the user's environment (with LC_ALL=C) and run unconfined;
  a trusted script can affect anything the user can.
- Previews for Pacman, Flatpak and Homebrew are estimates; races between preview
  and execution are detected afterwards, not prevented.
- Managed configuration files (ADR 013) are written as the user after review; a
  concurrent process can still change a target between the final check and the
  rename. Previews display file contents, so secrets in manifests are exposed to
  the terminal. This boundary was added after the review above; it has unit
  regressions and a Linux sandbox run, not a separate review.
- Flatpak system-scope authorization relies on Flatpak's system helper and polkit.
- The Flatpak local test repository is unsigned; signature handling against real
  remotes was not exercised.
