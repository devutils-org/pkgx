# Completion work and acceptance evidence

The v1 roadmap remains the product contract. A milestone is complete only with
code, regression tests, documentation and passing native-platform evidence.
There is no percentage-based substitute for those checks.

Current sequence:

1. Expose installed native dependency expressions through a common backend
   contract, CLI and TUI. Preserve alternatives/constraints and distinguish
   metadata from resolved dependency edges. Test with injected runners.
2. Complete local distribution tooling: versioned platform archives, checksums,
   installation guide and artifact verification. Do not publish automatically.
3. Advance the remaining ownership/receipt and native execution features only
   behind their existing confirmation and state-revalidation requirements.
4. Run the disposable Linux/macOS acceptance matrix where available; report
   missing runners explicitly. Never install/remove host packages for validation.

Strict synchronization now requires explicit APT management grants, a locked
registry snapshot and native removal scope checks (ADR 005). Ungranted identities
remain outside its authority. Script recipes remain data until
an execution/receipt design satisfies the product's ownership rules. Unavailable
native features stay unsupported rather than being silently substituted.
