# Support and validation status

| Environment | Local evidence | Native CI configured | Certified |
| --- | --- | --- | --- |
| Linux Rust stable | fmt, clippy, unit/integration fixtures | Ubuntu build/checks | No native version matrix yet |
| Debian Bookworm / Ubuntu 24.04 APT | Injected-runner tests | Disposable inspection + transactions | Pending CI |
| Arch Pacman | Fixtures + injected executor tests + disposable user-namespace Arch root: inspection and transactions (tests/sandbox/arch-userns.sh) | Disposable inspection + transactions | Pending CI |
| Fedora RPM/DNF | Native-output fixture tests | Disposable inspection | Pending CI |
| Flatpak default user/system | Scoped injected-runner tests | Not yet configured | No |
| macOS Homebrew | JSON fixture tests | macOS build + inventory smoke | Pending CI |
| Ratatui | TestBackend + Linux pseudo-terminal demo smoke, typed confirmation, worker cancellation | Unit tests | Native execution terminal/macOS checks pending |
| Explicit APT sudo | Injected credential/argv/order tests + harmless timeout/cancel subprocesses | Non-root disposable APT checks configured | PAM/terminal acceptance pending |
| Script recipes | Metadata + simulated checksum/tamper tests | Unit tests | Execution unsupported |

Docker/Podman and APT are absent in the local development sandbox. Pacman 7.1.0
on the host was used only for read-only inventory, dependency, provenance and
preview checks. Pacman transactions ran only inside a disposable root created by
tests/sandbox/arch-userns.sh in an unprivileged user namespace. No native host package transaction, downloaded script or cleanup operation
has been run. Configuring CI does not count as a successful CI run.

Build locally using Rust stable: `cargo build --release --locked -p pkgx-cli`.
Run fmt, workspace clippy with -D warnings and workspace tests before changes are
considered validated. Cargo.lock is checked in. `cargo run -p pkgx-cli -- completions
bash` (or zsh/fish) prints completion definitions; installation is explicit.

The manually triggered artifacts workflow builds/tests Linux and macOS binaries,
packages archives and emits SHA-256 checksum files. It uploads reviewable artifacts
but creates no GitHub release and publishes nothing. This workflow has not run here.
The workflow includes Linux host and musl archives plus macOS host archives;
these configured jobs still need actual execution. Local archives are built with
`scripts/package.py` and include version/target/build metadata and documentation. Signing/notarization is pending.

For comparable builds use the same Rust toolchain, target and Cargo.lock. A floating
stable toolchain and native linker do not guarantee byte-for-byte reproducibility.
Record `rustc -vV` and target details with any artifact before distributing it.

See [local validation checkpoint](validation.md) for exact commands, test counts and
which checks were not executed. Native evidence is not inferred from fixture success.
