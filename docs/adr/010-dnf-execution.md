# ADR 010: DNF5 preview and execution

Status: implemented experimentally; injected-runner regressions and a disposable
rootless Fedora 44 root (docs/validation.md). DNF4 is ADR 016.

Preview. `dnf5 install|remove --assumeyes --store=DIR` resolves the transaction
and writes it as `transaction.json` without running it; packages are downloaded
into DIR, a private temporary directory discarded after parsing. Only `rpms`
entries with Install/Upgrade (installs), Replaced (old side of an upgrade) or
Remove (removals) are accepted; groups, environments, modules or other actions
are refused. Upgrades are flagged as partial-upgrade risks. "Nothing to do" is a
verified no-op. Removal always uses `--no-autoremove`. DNF's protected packages
are enforced natively and fail the preview.

Execution never replays the stored transaction: `dnf5 replay` skips repository
OpenPGP checks for stored packages by default, which was observed natively. The
executor repeats the preview, refuses a stale plan, then runs
`dnf5 install --assumeyes <NEVRA>` (or `dnf5 remove --assumeyes --no-autoremove
<NEVRA>`) with each requested package pinned to its reviewed NEVRA, so repository
signature checks and native install reasons stay intact. Every planned NEVRA and
the untouched inventory are verified afterwards; differences are failures, and no
rollback is attempted.

Privilege. `--sudo` delegates only those two templates followed by valid NEVRAs to
`/usr/bin/dnf5`, which must be the binary selected on PATH.

The RPM inventory ignores `gpg-pubkey` key headers, which have no architecture;
any other architecture-less row still fails.
