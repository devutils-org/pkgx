# Completion work and acceptance evidence

The v1 roadmap remains the product contract. A milestone is complete only with
code, regression tests, documentation and passing native-platform evidence.
There is no percentage-based substitute for those checks.

Current sequence:

1. Done: Pacman executor (ADR 007), tracked script receipts (ADR 008) and
   conservative cleanup classes/execution (ADR 009), each with injected tests and
   disposable-root evidence (docs/validation.md).
2. Native APT acceptance in a rootless debootstrap root
   (tests/sandbox/debian-userns.sh), then the same scripts in configured CI.
3. Remaining executors in roadmap order (DNF, Flatpak, Homebrew) only with a native
   preview that can be revalidated; otherwise they stay unsupported.
4. Release qualification: signing, multi-platform artifact runs and macOS smoke.
   Never install/remove host packages for validation.

Strict synchronization now requires explicit APT management grants, a locked
registry snapshot and native removal scope checks (ADR 005). Ungranted identities
remain outside its authority. Script recipes execute only through
the receipt design in ADR 008. Unavailable
native features stay unsupported rather than being silently substituted.
