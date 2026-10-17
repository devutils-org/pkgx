# ADR 001: Identity, privilege boundaries and plan safety

Status: accepted for the foundation.

Package identity includes its provider and optional architecture or scope. Names alone
cannot identify packages across providers or dpkg multiarch installations.

Inspection uses argument-based subprocesses with a C locale, closed stdin, captured
output, timeout and cancellation. It never invokes a shell or privilege escalation.
Raw backend stderr is not printed to avoid leaking sensitive output. Executables
are resolved through PATH; users must supply a trusted PATH.

The execution policy is refined in ADR 002. An executor must validate the observed
state immediately before execution, present the native operation and its uncertainty,
and obtain explicit confirmation unless --yes is supplied. Privilege escalation
belongs only to that execution phase. Serialization of a Plan is not authorization.

Adapter creation now gates on recognizable versions at or above documented
implementation targets; this does not certify a native version matrix. APT inventory, search, native dependency expressions and simulation are
implemented. Inventory does not resolve dependency edges; an empty dependencies
vector does not prove absence of dependencies. Metadata labels the query status.
