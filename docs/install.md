# Build, verify and install

The project is version 0.1.0 and native execution remains experimental. There is
no published or certified v1 release. Build from the workspace with Rust stable:

```sh
cargo build --release --locked -p pkgx-cli
./target/release/pkgx --version
./target/release/pkgx demo
```

Use the actual Cargo target directory when CARGO_TARGET_DIR is configured.
`demo` needs no native package manager. `demo --snapshot --json` also works
without an interactive terminal. Native commands require the corresponding
provider binaries and a trusted PATH; see [support](support.md).

To install a locally reviewed binary for the current user:

```sh
mkdir -p "$HOME/.local/bin"
install -m 755 target/release/pkgx "$HOME/.local/bin/pkgx"
```

Add `$HOME/.local/bin` to your shell's PATH. Installing the CLI itself does not
require root. Native package transactions need native privileges; --sudo opts in
to delegating only a confirmed APT transaction. There is no implicit elevation. No bootstrap script is downloaded or executed.

## Reviewable archives

The packaging tool requires Python 3.11 or newer and uses only its standard
library. It never executes the binary it packages. Record the build compiler and
package the target you actually built:

```sh
rustc -vV > /tmp/pkgx-rustc.txt
build_target=$(rustc -vV | sed -n 's/^host: //p')
python3 scripts/package.py --binary target/release/pkgx \
  --target "$build_target" --rustc-info /tmp/pkgx-rustc.txt --output-dir dist
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
sha256sum -c pkgx-v0.1.0-TARGET.tar.gz.sha256    # Linux
shasum -a 256 -c pkgx-v0.1.0-TARGET.tar.gz.sha256 # macOS
```

Replace TARGET with the archive's actual target. A checksum detects byte changes;
it is not a signature or proof of publisher identity. The artifacts workflow
uploads build artifacts for review and does not publish a GitHub release.

## Shell completion

Bash: source `completions/pkgx.bash` from your shell configuration. Zsh: place
`completions/_pkgx` in a directory on `$fpath`, then initialize `compinit`. Fish:
copy `completions/pkgx.fish` to `~/.config/fish/completions/pkgx.fish`. Review files
before installing. `pkgx completions bash|zsh|fish` prints the same definitions.

## Uninstall the CLI

Remove only the CLI binary and completion files you explicitly installed. This
does not uninstall native packages or remove transaction history. Keep the private
journal for diagnosis; its configured location is described in [recovery](recovery.md).
