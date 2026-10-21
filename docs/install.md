# Build, verify and install

The project is version 0.1.0 and native execution remains experimental; 0.1.0 is
not a certified v1. Release archives are on the
[releases page](https://github.com/devutils-org/xpac/releases) (see
[Verify a release](#verify-a-release)). To build from the workspace with Rust stable:

```sh
cargo build --release --locked -p xpac-cli
./target/release/xpac --version
./target/release/xpac demo
```

Use the actual Cargo target directory when CARGO_TARGET_DIR is configured.
`demo` needs no native package manager. `demo --snapshot --json` also works
without an interactive terminal. Native commands require the corresponding
provider binaries and a trusted PATH; see [support](support.md).

To install a locally reviewed binary for the current user:

```sh
mkdir -p "$HOME/.local/bin"
install -m 755 target/release/xpac "$HOME/.local/bin/xpac"
```

Add `$HOME/.local/bin` to your shell's PATH. Installing the CLI itself does not
require root. Native package transactions need native privileges; --sudo opts in
to delegating only a confirmed APT transaction. There is no implicit elevation. No bootstrap script is downloaded or executed.

## Portable Linux builds

`sh scripts/distroproof/build.sh` builds and packages two x86_64 archives:
GNU linked against glibc 2.17 (Zig/cargo-zigbuild), and static musl. It checks
the GNU symbol floor and rejects a musl build needing a loader/shared library.
The [Distroproof guide](distroproof.md) lists the prerequisites, guest tests and
measured coverage. A normal `cargo build` uses the host's libc and does not
establish that portability floor. Both the matrix and Linux GNU artifact CI use
the portable build path; archives remain unpublished review artifacts.

## Reviewable archives

The packaging tool requires Python 3.11 or newer and uses only its standard
library. It never executes the binary it packages. Record the build compiler and
package the target you actually built:

```sh
rustc -vV > /tmp/xpac-rustc.txt
build_target=$(rustc -vV | sed -n 's/^host: //p')
python3 scripts/package.py --binary target/release/xpac \
  --target "$build_target" --rustc-info /tmp/xpac-rustc.txt --output-dir dist
```

For cross-compilation, pass the explicit target and matching target-directory
binary. The target label is supplied by the builder, not independently attested.
The archive contains the binary, BUILD.json, both MIT and Apache-2.0 licenses,
documentation, example
manifest files and shell completions. BUILD.json records the binary and Cargo.lock SHA-256, target, version,
compiler information when supplied, and the explicit absence of certification.
Archive and checksum destinations are never overwritten.

Set SOURCE_DATE_EPOCH (or --epoch) to the chosen source timestamp. The default is
zero, not the current clock. Equal files, target, compiler record and epoch produce
identical archive bytes: file ordering, permissions, owners and gzip/tar timestamps
are normalized. This does not make independently compiled Rust binaries identical.
Use the same toolchain, target, lockfile, source and native linker to compare builds.

Verify a checksum obtained through a trusted channel before extraction:

```sh
sha256sum -c xpac-v0.1.0-TARGET.tar.gz.sha256    # Linux
shasum -a 256 -c xpac-v0.1.0-TARGET.tar.gz.sha256 # macOS
```

Replace TARGET with the archive's actual target. A checksum detects byte changes;
it is not a signature or proof of publisher identity. A manual run of the
artifacts workflow uploads build artifacts for review and publishes nothing.

## Verify a release

Pushing a version tag runs the same workflow, which also records a Sigstore build
provenance attestation for each archive and opens a draft GitHub release with the
archives, their `.sha256` files and `SHA256SUMS`. The draft is published by hand
after review. The attestation ties the archive's bytes to this repository, the
workflow and the tagged commit:

```sh
gh attestation verify xpac-v0.1.0-TARGET.tar.gz --repo devutils-org/xpac
```

The macOS binary is not notarized by Apple; a browser download is quarantined by
Gatekeeper (`xattr -d com.apple.quarantine xpac` after verifying it).

## Shell completion

Bash: source `completions/xpac.bash` from your shell configuration. Zsh: place
`completions/_xpac` in a directory on `$fpath`, then initialize `compinit`. Fish:
copy `completions/xpac.fish` to `~/.config/fish/completions/xpac.fish`. Review files
before installing. `xpac completions bash|zsh|fish` prints the same definitions.

## Uninstall the CLI

Remove only the CLI binary and completion files you explicitly installed. This
does not uninstall native packages or remove transaction history. Keep the private
journal for diagnosis; its configured location is described in [recovery](recovery.md).
