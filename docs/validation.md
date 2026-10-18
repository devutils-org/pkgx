# Local validation checkpoint — 2026-10-18

Environment: Linux x86_64 (Arch), rustc 1.99.1 (58cf8f9c3 2026-10-13), Python 3.14.
Cargo output uses /tmp/pkgx-target. Earlier checkpoints ran on Linux aarch64.

## Automated checks

```sh
cargo fmt --check
CARGO_TARGET_DIR=/tmp/pkgx-target cargo clippy --workspace --all-targets --locked --offline -- -D warnings
CARGO_TARGET_DIR=/tmp/pkgx-target cargo test --workspace --locked --offline -q
CARGO_TARGET_DIR=/tmp/pkgx-target cargo build --release --locked --offline -p pkgx-cli
python3 scripts/test_package.py
```

184 Rust tests passed, plus 2 Python packaging tests. One standalone ignored Rust
test is a subprocess fixture explicitly invoked by the inherited-pipe regression;
it does not represent missing product coverage.

Coverage includes native inventories/marks, dependency expressions, provider and
architecture identities, manifest composition, explicit management grant storage,
strict removal scope, lock contention/stale state, shared/symlinked paths, native
cascades, saved-plan expiry, held/critical/purge rejection, unexpected native effects,
partial failures, journals, recipe tampering, managed configuration files (states,
writes, adoption, symlink and stale-review refusals) and TUI confirmation/navigation.
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

## Disposable Arch root — 2026-10-18

Environment: Linux x86_64 Arch host, Pacman 7.1.0 / libalpm 16.0.1, bubblewrap 0.13.1.
`tests/sandbox/arch-userns.sh WORKDIR target/debug/pkgx` bootstrapped a fresh Arch
root (108 packages, signatures required, public keyring copied) with every pacman
path redirected into WORKDIR, then ran `tests/containers/pacman.sh` and
`tests/containers/pacman-transactions.sh` with that root bound as `/` inside an
unprivileged user namespace. Both passed (exit 0):

- read-only providers/list/info/why/deps/audit/export/diff/plan;
- unconfirmed noninteractive apply refused with no change;
- manifest install of `sl` with explicit reason, verified versions and journal;
- repeated apply returned an empty operation list;
- IgnorePkg refusal by pkgx policy;
- HoldPkg and critical-package removal previews refused (in this run pacman's own
  noninteractive HoldPkg prompt and dependency checks refused first; the pkgx
  policy path is covered by injected-runner tests);
- a saved removal plan invalidated by an inventory change was refused;
- a fresh saved removal plan removed `sl` without cascading into `ncurses`;
- an `--asdeps` package was reported as `suggested_cleanup`, an unconfirmed
  `clean --orphans` was refused, and a confirmed one removed it without touching bash;
- `clean --cache` ran `pacman -Sc` and verified the cache shrank;
- history and doctor reported the journaled transactions with successful results;
- the disposable root was removed afterwards (packages stay in WORKDIR/cache).

The first run found a real defect: `export` of a Pacman inventory produced a
manifest that `diff`/`plan` rejected because the `pacman` package looked like a
redundant provider prefix (the same affected `apt` on Debian). It is fixed and
covered by a regression test. The host package database was never a target: host
files owned by the real root are not writable from the namespace, as checked
before the run. Two bootstrap-only hooks failed inside the namespace; they are
unrelated to the checked transactions. --sudo was not exercised.

## Rootless Debian, Ubuntu and Fedora roots — 2026-10-18

`tests/sandbox/debian-userns.sh` and `tests/sandbox/fedora-userns.sh` build a
disposable Arch tool root (debootstrap + Debian/Ubuntu keyrings, or dnf5), then a
target root on a tmpfs inside a user namespace mapping the subordinate UID range,
with a static musl pkgx. Host package databases were never targets.

- Debian bookworm (APT 2.6.1) and Ubuntu 24.04 noble: `inspection.sh`,
  `transactions.sh` and `privilege.sh` passed — inspection, export/diff/plan,
  confirmed install, idempotent re-apply, hold and Essential refusals, unconfirmed
  refusal, saved manual-mark plan, stale saved plan refusal, strict-mode grant and
  removal, and `--sudo` refusal without policy followed by delegated install/removal
  under a NOPASSWD apt-get rule with private 0700/0600 journals. `privilege-pam.sh`
  (bookworm) drove the interactive path through a pseudo-terminal with a
  password-required rule: a wrong password was refused before any transaction,
  and the right one authorized a verified install and removal.
- Fedora 44 (dnf5 5.4.1): `rpm.sh` and `dnf-transactions.sh` passed — read-only
  checks, confirmed install with native user-installed reason, idempotent re-apply,
  protected-package refusal, stale saved plan refusal and reviewed removal.

Defects found and fixed by these runs: Debian packages named `apt`, `flatpak` or
`dnf` were misparsed or rejected by the APT identity parser; RPM inventories broke
after the first signed install because of `gpg-pubkey` key headers; the Debian
root directory created by mktemp was not traversable (sandbox script). It was also
observed that `dnf5 replay` skips OpenPGP checks for stored packages, which is why
the DNF executor never replays (ADR 010). The scratch /tmp is nosuid, so target
roots live on a namespace tmpfs where setuid sudo works.

## Rootless Flatpak root — 2026-10-18

`tests/sandbox/flatpak-userns.sh` installs Flatpak 1.18.4 into a disposable Arch
root and runs `tests/containers/flatpak-transactions.sh`, which builds a tiny
runtime and app into a local unsigned repository (no remote contacted). Passed:
preview naming the local remote, full commits and the missing runtime; unconfirmed
install refused; manifest apply installed app and runtime with verified origin and
commit; repeated apply was empty; runtime removal refused; app removal kept the
runtime; after adding a second remote offering the same ref, installation was
refused as ambiguous. Export triggers fail inside the namespace (nested bwrap) and
were ignored by Flatpak itself.

## Rootless Homebrew root — 2026-10-18

`tests/sandbox/brew-userns.sh` runs `tests/containers/brew-transactions.sh` as an
unprivileged user in a disposable Arch root after cloning Homebrew 7.0.9 (latest
tag). Passed: preview of jq plus oniguruma, unconfirmed install refused, manifest
apply installed both with verified versions, repeated apply empty, oniguruma
reported as a native dependency, removal of a required formula refused, jq removal
kept oniguruma, the audit reported oniguruma and `clean --orphans` removed it, and
the journal recorded the results. Two defects were found and fixed on the way:
Homebrew 7 receipts lack `installed_as_dependency` (reasons were `unknown`), and
`brew uninstall` autoremoves dependencies unless HOMEBREW_NO_AUTOREMOVE is set.

The same check now refuses to run unless PKGX_DISPOSABLE=1 and uses a preinstalled
`brew` when present, so the `macos-smoke` CI job runs it on the discarded hosted
runner. It was re-run in the Linux root after that change (passed); it has not run
on macOS yet.

## Script recipe sandbox — 2026-10-18

`tests/sandbox/recipe-userns.sh target/debug/pkgx` runs `tests/sandbox/recipe-checks.sh`
in bubblewrap with the host bound read-only, private tmpfs /tmp and HOME, and a
loopback-only network namespace. It serves its own fixture script over HTTPS
(self-signed CA via CURL_CA_BUNDLE) and passed: tampered digest refused before
execution; unconfirmed install refused; confirmed install created and recorded a
file, a symlink and a configuration file (`tracked`); duplicate install refused; a
user-modified owned file blocked uninstall without running anything; uninstall
removed owned resources, retained the edited configuration and dropped the receipt;
a pre-existing user file could not be claimed by a new install. With `--observe`,
the fixture's undeclared `~/.cache/demo-tool.log` was reported, the receipt was
classified `observed`, and the log survived uninstall.

## Managed configuration file sandbox — 2026-10-18

`tests/sandbox/files-userns.sh target/debug/pkgx` runs `tests/sandbox/files-checks.sh`
in bubblewrap with the host bound read-only, private tmpfs /tmp (HOME and state
directory) and no network. It passed: an unmanaged `~/.bashrc` blocked `apply` with
no file or state created; adoption without --yes was refused, then stored a private
0600 backup in a 0700 registry; `apply` replaced the adopted file, created
`~/.config/tmux/tmux.conf` with mode 0644 and 0700 parents, left no temporary file
and was a no-op when repeated; a local edit was reported as drift and kept, and
became in sync once copied into the source; a symlinked target, symlinked ancestor
and symlinked source were refused without touching the outside files; a held lock
refused writes; `file remove` refused a drifted file and deleted an unchanged one
with its receipt; `file forget` kept the file. Interrupted writes and running as root
were exercised only by unit-level reasoning, not in the sandbox.

A Linux pseudo-terminal smoke of `tui --provider pacman -f` (read-only host Pacman
inventory, temporary HOME) showed `unmanaged_conflict ~/.bashrc` and `missing
~/.tmux.conf` in the manifest review (m) and wrote no file or state.

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

Homebrew casks, cross-provider atomic execution and automatic cleanup beyond native
cache commands and reviewed orphan removal remain unsupported. Sudo authentication
passed through a pseudo-terminal in a disposable Debian root; TUI terminal
suspension during the sudo prompt is covered only by unit tests. The
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
