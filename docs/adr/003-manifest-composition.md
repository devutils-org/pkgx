# ADR 003: Additive composition and provider boundaries

Status: implemented with fixture evidence; native matrix pending.

Manifest v1 uses a strict data-only schema. Base packages, the matching platform
layer and explicitly selected profiles are combined through a stable additive union.
Profile ancestors come first in declared extends order. Selection order is explicit,
duplicates across layers collapse and no profile can implicitly remove a package.
Unknown inactive layers still fail validation; nonmatching known platforms are
reported as skipped. Inheritance cycles and unresolved profile names are errors.

Names carry a provider and architecture/scope. Brew kind and Flatpak ref/kind/scope
are mandatory to avoid collisions. A native mark does not prove xpac ownership.
Strict synchronization was initially refused. ADR 005 now defines explicit
APT management grants and fail-closed native removal scope; platform acceptance
remains pending.

Mixed-provider planning checks all requested providers and only asks adapters for
capabilities they implement. Unsupported missing-package operations fail. Execution
is currently APT and Pacman, with preflight of every manifest provider before a
transaction is considered; each provider gets its own reviewed, confirmed transaction
in stable provider order and the first failure stops later providers. No partially applied multi-provider transaction is represented as
atomic. A fully satisfied mixed manifest is a read-only no-op.

Script recipe declarations are not ownership receipts. Preparation verifies integrity
but never runs interpreters or deletes declared paths. Execution is deferred until
its trust/receipt and sandbox boundaries can be validated in disposable environments.
