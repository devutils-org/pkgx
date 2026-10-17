# Local validation checkpoint — 2026-10-17

Environment: Linux aarch64, rustc 1.99.0 (b940084d7, 2026-09-28), Python 3.14.8.
Cargo output uses /tmp/pkgx-target because the shared default target is read-only.

## Automated checks

```sh
cargo fmt --check
CARGO_TARGET_DIR=/tmp/pkgx-target cargo clippy --workspace --all-targets --locked --offline -- -D warnings
CARGO_TARGET_DIR=/tmp/pkgx-target cargo test --workspace --locked --offline -q
CARGO_TARGET_DIR=/tmp/pkgx-target cargo build --release --locked --offline -p pkgx-cli
python3 scripts/test_package.py
```

134 Rust tests passed, plus 2 Python packaging tests. One standalone ignored Rust
test is a subprocess fixture explicitly invoked by the inherited-pipe regression;
it does not represent missing product coverage.

Coverage includes native inventories/marks, dependency expressions, provider and
architecture identities, manifest composition, explicit management grant storage,
strict removal scope, lock contention/stale state, shared/symlinked paths, native
cascades, saved-plan expiry, held/critical/purge rejection, unexpected native effects,
partial failures, journals, recipe tampering and TUI confirmation/navigation.
A mocked strict APT transaction revalidates its native plan, verifies removal,
persists its outcome and holds the registry lock. Another test edits the registry
during native revalidation and confirms no transaction is started.

Packaging tests compare two byte-identical archives, verify the checksum and build
manifest, inspect normalized archive metadata and refuse overwrite, symlink inputs
and traversal targets. Deterministic packaging is not a claim that independently
compiled binaries are byte-identical.

Bash/Zsh completions and POSIX container scripts pass syntax checks without native
transaction execution. Workflow YAML syntax is checked separately from execution.
Fish is unavailable, so its completion definition has not been parsed by Fish.

## Native and terminal evidence

Pacman 7.1.0 / libalpm 16.0.1 is available in this environment. The actual pkgx binary
successfully read its 301-package inventory and queried installed dependency
expressions and native installation provenance for bash. These operations were
read-only. This is consultation evidence, not Arch transaction certification.

Linux pseudo-terminal smoke tests exercised the demo's provider focus, search,
package info, help overlay, Home/End, preview toggling and Shift-Tab, including a
150×25 terminal. Terminal restoration succeeded on exit. TestBackend tests cover
small terminals, wrapped Unicode details and stable scrolling through 80 packages.
The help overlay preserves reviewed plans and consumes action keys without arming
execution. No real native transaction was driven through the terminal.

## Pending evidence and unsupported work

Docker/Podman and APT are unavailable locally. Configured Debian/Ubuntu transaction
checks (including strict mode/idempotence), disposable Arch/Fedora consultation,
macOS Homebrew smoke and multi-platform artifact jobs have not run here. Configuring
these jobs is not evidence of passing them. No native package transaction,
downloaded installation script or cleanup operation has run on the host.

Non-APT executors, script installation/uninstall/receipts, cross-provider strict
execution and automatic cleanup remain unsupported. Explicit sudo delegation is
implemented; its native authentication/terminal validation remains pending. The
source/security review in security-review.md is not an independent audit. No
native-platform certification or v1.0 release is claimed.

## Explicit privilege delegation

The --sudo path is covered by injected credential/runner tests: no authentication
without opt-in/confirmation or for no-ops, authentication refusal/cancellation,
fixed executable and argv delegation, unchanged unprivileged inspection, and
rejection of alternate APT executables/database overrides. A mocked full APT
transaction asserts authentication happens before native revalidation, and that a
transaction-time sudo refusal does not trigger another prompt. Harmless sleep
children verify bounded authentication cancellation/timeout and reaping. CLI tests
refuse --sudo on inspection/read-only TUI and apply an empty manifest without
credentials, HOME, a native tool, or state writes.

No sudo invocation ran on the host. tests/containers/privilege.sh is configured for
disposable Debian/Ubuntu non-root execution, but has only been syntax-checked here.
The interactive PAM prompt and TUI terminal suspension still need native evidence.

Cooperative termination and bounded cleanup also have harmless-child regressions.
The delegate receives a three-second relay grace period; synchronous reaping is
bounded to 200 ms with an explicit failure/background reaper if a child does not
exit. This does not establish that all privileged descendants stop on cancellation.
The existing unreaped-leader/inherited-pipe regression still passes.
