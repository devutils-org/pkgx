# ADR 007: Pacman execution

Status: implemented experimentally; injected-runner regressions and a disposable
user-namespace Arch root (see docs/validation.md). Arch CI container run pending.

Pacman is the second provider with an executor, behind the same contract as APT
(ADR 002): reviewed preview, explicit confirmation or --yes, independent native
revalidation after confirmation, private journal intent/result and post-transaction
verification. There is no provider fallback; one transaction targets one provider.

Preview. `pacman --sync --needed --print` (installs) or `pacman --remove --print`
(removals) remains an explicitly uncertain estimate: hooks, conflict replacements
and scriptlets are not modeled. Mixed install/remove requests are refused. An
already-installed request is a verified no-op and its install reason is unchanged.

Policy, checked at preview and again during revalidation:

- Removal of a HoldPkg match, or of the built-in critical set (pacman, glibc,
  filesystem), is refused.
- Installation is refused when any previewed package matches IgnorePkg, or when
  IgnoreGroup is non-empty (group membership is not evaluated).
- Glob syntax other than `*` and `?` in these lists fails closed.

These refusals replace prompts that `--noconfirm` would otherwise answer with a
default. Remaining prompts (conflicts, provider choice) default to aborting or to
pacman's first choice; the latter is only detectable afterwards through verification.

Execution. Arguments are rebuilt from validated identities, never from a saved
command: `pacman --sync --needed --noconfirm --noprogressbar --color never --
<repo>/<name>` pinning the reviewed repository, or `pacman --remove --noconfirm
--noprogressbar --color never -- <name>`. No `--refresh`, `--sysupgrade`,
`--recursive`, `--cascade`, `--nosave` or alternate root/config/database options
are ever produced. Dependency upgrades in a preview are partial-upgrade risks that
the user must review; pkgx never refreshes sync databases.

Pacman cannot pin versions. Verification compares each installed version with the
reviewed `--print` version and fails if any planned action or untouched package
differs. A failure never implies rollback.

Privilege. `--sudo` authenticates after confirmation and delegates only argument
vectors equal to the two templates above followed by non-option targets, to the
fixed `/usr/bin/pacman`, which must be the binary selected on PATH. Inspection,
pacman-conf and journal writes stay unprivileged.

Native interface references:

- [pacman(8)](https://man.archlinux.org/man/pacman.8)
- [pacman.conf(5)](https://man.archlinux.org/man/pacman.conf.5)
