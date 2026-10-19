# ADR 014: apk preview and execution

Status: implemented experimentally; injected-runner regressions
(`crates/xpac-core/tests/apk_exec.rs`) and disposable Alpine 3.24 guests
(apk-tools 3.0) in Distroproof, with apk-tools 2.14 output checked in an
Alpine 3.22 container. apk-tools before 2.12 is refused.

Inventory. `apk list --installed` gives `name-version-rN arch {origin}`; apk
versions hold no hyphen but the one before the mandatory release, so the name
is everything before the second hyphen from the end, and anything else fails.
The license column may hold spaces and parentheses and is not read. A package
is explicit when a constraint in `/etc/apk/world` names it; apk installs nothing
else, so every other package is a dependency. Conflicts (`!foo`) and provides
(`so:`, `cmd:`) in WORLD name no package.

Preview. `apk add|del --simulate` resolves against the local indexes without
root or the database lock and lists `(i/n) Installing|Upgrading|Purging name
(version)`. Installs accept Installing and Upgrading (flagged as a
partial-upgrade risk); removals accept Purging. Any other action is refused. A
request already installed as a dependency is a `MarkExplicit` operation: `apk
add` records it in WORLD without reinstalling. `apk del` of a package installed
packages need prints "World updated, but the following packages are not removed"
and exits 0 having edited WORLD alone; the preview refuses it, naming them. `apk
del` also purges the dependencies nothing needs any more; each is listed and
reviewed. A requested architecture other than `apk --print-arch` is refused.

Execution. apk cannot pin a version without recording the constraint in WORLD
for good (`apk add foo=1.0-r0` stays a pin), so the executor repeats the preview,
refuses a stale plan, then runs `apk add --no-interactive --no-progress -- NAME...`
(or `apk del ...`) by name, never with `--update-cache`, `--upgrade`, `--force*` or
`--allow-untrusted`, and verifies every reviewed version, explicit mark and the
untouched inventory afterwards. Differences are failures; no rollback is attempted.

Privilege. `--sudo` delegates only those two templates followed by non-option
targets to `/sbin/apk`, which must be the binary selected on PATH.
