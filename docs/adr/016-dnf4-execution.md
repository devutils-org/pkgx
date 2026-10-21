# ADR 016: DNF4 preview and execution

Status: implemented experimentally; injected-runner regressions
(`crates/xpac-core/tests/dnf_exec.rs`), disposable Rocky Linux 8 (DNF 4.7) and 9
(DNF 4.14) guests in Distroproof, and a disposable Rocky 9 container in CI.

Context. RHEL 8 and 9 and their rebuilds (Rocky, AlmaLinux) run DNF4, where
`dnf` is `dnf-3`, written in Python. ADR 010 executes only through DNF5, whose
`--store` writes a resolved transaction as JSON without running it; DNF4 has no
such option, and its `--assumeno` transaction table is text meant for people.

Generation. `dnf --version` names it: `dnf5 version 5...` is DNF5, a bare `4...`
is DNF4. Every preview and execution reads it again; nothing is cached.

Preview. DNF4's own resolver computes the transaction through the `dnf` module
(`providers::DNF4_RESOLVE`, run by `/usr/libexec/platform-python` where it
exists, else `python3`): the configuration and `/etc/dnf/vars` as `dnf` reads
them (Rocky 9's mirror list needs `$rltype`), the installed and enabled
repositories, `install` or `remove` of the requested specs with
`clean_requirements_on_remove` off, and `resolve(allow_erasing=False)`. Nothing
runs and nothing but metadata is downloaded. Each transaction item is printed as
dnf5's stored transaction spells it -- NEVRA with its epoch, Install, Upgrade,
Replaced (the old side of an upgrade), Remove, the reason and the repository --
and goes through the same parser as dnf5's: any other action (obsoletes,
downgrades, reinstalls) is refused. Protected packages fail the resolution, as
with `dnf`. Unprivileged, the metadata cache is the user's, as `dnf` uses.

Execution. As ADR 010: the preview is repeated and a stale plan refused, then
`dnf install --assumeyes <NEVRA>...` or `dnf remove --assumeyes --noautoremove
<NEVRA>...` with each request pinned to its reviewed NEVRA, so repository
signature checks and native install reasons stay intact, and every planned NEVRA
and the untouched inventory are verified afterwards. No rollback.

Privilege. `--sudo` delegates only those two templates followed by valid NEVRAs
to `/usr/bin/dnf`, which must be the binary selected on PATH; where
`/usr/bin/dnf5` exists, DNF5's templates and binary are the only ones accepted.

Consequences. A Python program runs as part of a preview, but it is a constant
compiled into xpac, given only the operation and validated package specs as
arguments, and run unprivileged. Module streams and groups are not requested and
not supported.
