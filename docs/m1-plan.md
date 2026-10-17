# M1/M2 continuation plan

Implement the next vertical slice: capability-based backend interface, validated
APT identities, search/info/why, protected native simulation, and strict v1 TOML
parsing with additive export/diff/plan. An experimental executor now revalidates plans, pins requested versions and verifies
results; native transaction validation in disposable environments remains outstanding.

Tests use injected runners; neither Docker nor Podman is installed here. Add a
container CI job for native inspection evidence, but do not count unrun CI as passed.
No native package transaction will run on the workstation.

Safety: reject APT selectors, globs, options, paths and interpolation as names.
Preserve raw dependency expressions instead of inventing resolved dependency edges.
Simulation must expose every Inst/Remv/Conf action, honor holds, and reject removal
of Essential/Protected packages. Plans remain uncertain; execution requires explicit confirmation and revalidation.

Declarative state: version 1; additive default; unknown keys and script package sections fail with explicit errors; implemented
providers have scoped identities and platform/profile layers resolve deterministically. Strict reconciliation refuses until managed ownership
exists; native manual marks do not prove pkgx ownership. No undeclared removals.
