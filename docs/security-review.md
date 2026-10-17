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
- Recipe resource declarations remain unverified ownership. Audit never traverses
  arbitrary data paths, purges configurations or executes native cleanup.
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
or script execution sandbox exists. Explicit APT sudo delegation is implemented;
its native PAM/policy/terminal acceptance remains pending. APT's own lock
covers its transaction, not the earlier simulation; races can be detected afterward,
not prevented atomically. Native command cancellation can leave partial changes.

The configured Debian/Ubuntu/Arch/Fedora/macOS jobs have not run locally. TUI widget,
confirmation and worker-cancellation logic is tested. Linux pseudo-terminal demo
smoke tests and native read-only Pacman checks passed; macOS terminal and real native
transaction validation remain pending. Script install/uninstall, script receipt
ownership, cross-provider strict execution and automatic cleanup stay unsupported.
