# ADR 005: Explicit management grants and strict APT scope

Status: implemented with injected-runner evidence; native container acceptance pending.

Native installation marks and existing packages do not prove that xpac was asked
to manage them. Installing, exporting or observing a package never grants it to
xpac implicitly. `manage apt:package:architecture` is a separate user decision:
after preview and confirmation it records the concrete native identity, observed
version and installation reason. `manage --forget` revokes that delegation without
querying a native tool or uninstalling the package. Neither action claims file
ownership. Other providers and scripts cannot receive these APT grants.

The private versioned JSON registry lives below `<state-dir>/managed/registry.json`.
It is separate from journal entries. Reads create nothing. Writes use a private
exclusive lock file, atomic rename and fsync. A crash can leave a lock; xpac refuses
automatic lock removal. Unknown fields, malformed state, duplicate identities,
non-private paths, symlinks and snapshots changed since review fail closed.

A strict manifest can remove only undeclared granted identities in its explicit
APT section. Omission of a provider section does not mean remove that provider.
An empty APT section explicitly selects its managed scope. All other observed
packages are retained. Changed native version/reason requires a fresh reviewed
grant. The complete native removal plan must be contained in the proposed removal
set: cascades removing ungranted packages or still-desired granted packages are
refused. Holds and Essential/Protected checks remain native-planner requirements.

After confirmation, CLI/TUI hold a registry writer lock through native revalidation,
journaling and execution. The registry snapshot is checked at every executor guard
point, including immediately before native execution. Native inventory and the
whole plan still need to match. Grant validation does not replace those checks.

A grant records explicit authority, not a live installation receipt. Removed or
externally modified packages can leave historical grants in the registry. Missing
packages generate no removal; changed versions/reasons cannot be removed by the
old grant. Revocation is explicit. New installations require a separate grant if
the user wants later strict removal. Cross-provider strict execution, automatic
adoption, filesystem removal and script ownership inference remain unsupported.
