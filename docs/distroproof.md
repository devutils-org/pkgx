# Linux portability with distroproof

The integration follows Omniscient's installed-release matrix: distroproof boots
an official image on its own kernel, installs the archives, records envprobe,
runs the checks as the configured user, and brings `cell.json` back with `collect`.
The host judges the cell against the configured targets and the current builds.
Nothing runs a native package transaction on the host.

## Reproduce

Host prerequisites: Rust with `x86_64-unknown-linux-musl`, Zig 0.16,
`cargo-zigbuild`, Python 3.11+ (for the host judge), `readelf`, and distroproof
with `collect` support (tested from `412d873`). Keep its static `envprobe` beside
its binary. The guest checks support Python 3.6+, including Rocky 8's
`/usr/libexec/platform-python`; setup installs Python only if needed in a guest.
Distroproof needs QEMU, qemu-img, curl and OVMF; KVM accelerates the VMs.

```sh
export CARGO_TARGET_DIR=/tmp/xpac-target
rustup target add x86_64-unknown-linux-musl
cargo fetch --locked
sh scripts/distroproof/build.sh
mkdir -p dist/matrix
distroproof check
distroproof up --no-tui --jobs 4 --json dist/matrix/inspection.json
python3 scripts/distroproof/check.py dist/matrix/inspection.json \
  --output dist/matrix/inspection-cells

# Real installations/removals, ONLY inside new disposable VM overlays.
distroproof up -f distroproof-transactions.toml --no-tui --jobs 4 \
  --json dist/matrix/transactions.json
python3 scripts/distroproof/check.py dist/matrix/transactions.json \
  --matrix distroproof-transactions.toml --output dist/matrix/transaction-cells
```

`--offline` on `distroproof up` uses its cached images; it does not cut the guest's
network. Inspection normally needs no package repositories except to install
Python on small images. The transaction matrix downloads guest packages. Debian 10 setup points its
throwaway guest at Debian's signed buster archive because the default mirrors
no longer carry it; signature verification stays enabled.
Builds use Cargo's cache with `--offline` after the explicit fetch.

For a pilot use `distroproof up --only debian-12,alpine ...` and pass the same
`--only debian-12,alpine` to `check.py`. A partial report is otherwise a failure.

## What is checked

Two archives are produced with the existing deterministic packager:

- GNU, linked against glibc **2.17** with Zig, with a `readelf` symbol-floor check;
- musl, checked for no dynamic interpreter and no needed shared library.

The GNU archive is not applicable on a musl-only Alpine image; the cell records
why and checks the musl archive instead. The rest of the matrix checks both
archives. The symbol floor does not mean glibc 2.17 itself was booted: the oldest
measured userspace here is Ubuntu 18.04's glibc 2.27.

Each applicable build checks version, provider detection, doctor, synthetic demo,
three completions, history without creating state, and managed files. The latter
runs the existing private-HOME file checks as a regular user; root is instead
required to refuse file adoption explicitly.

For every detected supported APT/Pacman/DNF provider it also checks:

- the complete inventory against a separate native query (name, architecture,
  version and multiplicity), not just a non-empty JSON document;
- installed search, info, installation reason and dependencies;
- export → diff with no missing identity → plan with no package operations.

Every absent or too-old provider must fail explicitly with a versioned error.
Alpine and Gentoo exercise the apk and Portage providers (ADR 014, ADR 015): their
setup refreshes the package indexes the images lack, as a user does once, and changes
no package. DNF4 on Rocky is tested for RPM consultation; native
transaction execution still needs DNF5. Flatpak and Homebrew are not installed
by this matrix; their existing sandbox/macOS checks remain separate.

The transaction matrix uses the existing APT, Pacman and DNF5 regression scripts
against the installed **musl** release. They exercise confirmed installation,
idempotent reapply, protections, stale saved plans and removal. APT additionally
checks unprivileged sudo refusal followed by explicitly delegated operations.
GNU transaction execution is not independently exercised by these scripts.

This is Linux **x86_64** evidence. It does not establish aarch64, macOS,
interactive TUI behavior, arbitrary native versions, or every solver scenario.
The existing platform-specific tests and `certified: false` remain relevant.

## Judging evidence

`check.py` rejects a failed VM/setup/command, missing or duplicate target, missing
cell or check, wrong schema, stale binary digest, inventory witness with no
packages, or an unjustified build exclusion. Kernel, UID and native provider
presence are compared with envprobe's independent report. A build declaring
success cannot hide an individual failed check. The raw command output is kept
inside the cell for diagnosis.

The judge copies cells and envprobe reports into `dist/matrix/*-cells/`, with a
`summary.json`. Raw distroproof reports retain the image filename, setup/output,
variant, kernel and duration. The reports and archives are local artifacts,
ignored by Git; the concise measured checkpoint is below. `test_check.py` plants
missing checks, wrong kernels/UIDs, failed commands and stale digests to ensure
the judge rejects them.

`.github/workflows/distroproof.yml` is a manual workflow using the pinned
Distroproof implementation. It builds both archives, runs both matrices and
uploads the reports/cells even on failure. Preparing the workflow is not a
claim that a hosted CI run has passed.

## Findings and measured results

The first inspection sweep showed that the binary started on the old images,
but the APT 2.6 policy rejected Debian 10/11 and Ubuntu 18.04/20.04/22.04 before
consultation. The policy is now APT **1.6**. A fresh sweep exercised native
inventories, dependencies and export/plan roundtrips on those actual versions;
the transaction matrix separately verifies native execution.

The harness itself needed three corrections exposed by the guests: Rocky 8
keeps Python outside PATH; a Distroproof root command has no login HOME; and
managed-file writes as root must be tested as refusals, not expected to succeed.
These were test-harness defects, not reasons to relax xpac's root-write policy.

The initial native archive built against the host libc was replaced for this
matrix by the glibc 2.17 build, and Linux GNU artifact CI now uses the same floor.
The musl build is a separate static archive; neither archive is published here.

<!-- The checkpoint table is generated from the judged local reports. -->

### Inspection checkpoint — 2026-10-19

18/18 environments passed, with 35 applicable archive/guest combinations and
749 recorded checks. The GNU archive on musl-only Alpine is explicitly not
applicable. Guest UID, kernel and provider presence agree with envprobe.

| Target | Observed kernel | Supported native provider | Result |
|---|---|---|---|
| debian-10 | 4.19.0-27-amd64 | APT 1.8.2.3 | pass |
| debian-11 | 5.10.0-46-amd64 | APT 2.2.4 | pass |
| debian-12 | 6.1.0-54-amd64 | APT 2.6.1 | pass |
| debian-13 | 6.12.113+deb13-amd64 | APT 3.0.3 | pass |
| debian-sid | 7.2.11+deb14-amd64 | APT 3.3.3 | pass |
| ubuntu-18.04 | 4.15.0-212-generic | APT 1.6.17 | pass |
| ubuntu-20.04 | 5.4.0-216-generic | APT 2.0.11 | pass |
| ubuntu-22.04 | 5.15.0-198-generic | APT 2.4.14 | pass |
| ubuntu-24.04 | 6.8.0-147-generic | APT 2.8.3 | pass |
| rocky-8 | 4.18.0-553.el8_10.x86_64 | DNF 4.7.0 | pass |
| rocky-9 | 5.14.0-687.10.1.el9_8.0.1.x86_64 | DNF 4.14.0 | pass |
| fedora-previous | 6.17.1-300.fc43.x86_64 | DNF 5.2.17.0 | pass |
| fedora-latest | 6.19.10-300.fc44.x86_64 | DNF 5.4.1.0 | pass |
| arch | 7.2.9-arch1-1 | Pacman 7.1.0 | pass |
| gentoo | 6.18.57 | none (explicit refusals) | pass |
| alpine | 6.18.35-0-virt | none (explicit refusals) | pass |
| debian-12-root | 6.1.0-54-amd64 | APT 2.6.1 | pass |
| fedora-lockdown | 6.19.10-300.fc44.x86_64 | DNF 5.4.1.0 | pass |

The checked binary SHA-256 digests for this checkpoint are:

```text
gnu   2c287c896af8441f42b0a503f367e454234ca022a1b6182a2a9d93b77ab1af91
musl  eabc48bc2b49d2a64d5e6f28543e32d1cac636bda0dba7f4b1fa8bde99cc35f0
```

Host build: Rust 1.99.1, Zig 0.16.0, cargo-zigbuild 0.23.4. Inspection was run
as an ordinary guest user except for the explicit Debian root variant. The
Fedora lockdown variant also passed. These are measured observations for the
recorded images/builds, not blanket certification of newer native versions.

### Transaction checkpoint — 2026-10-19

10/10 fresh disposable VM targets passed in one final sweep, after adapting
Debian 10's archived repository locations. All 20 archive/guest inspection
combinations passed (440 checks), followed by 17 native regression scripts
using the installed static musl release. Their UID/kernel/provider checks also
agree with envprobe. The raw final report is `dist/matrix/transactions-final.json`;
the judged cells and summary are in `dist/matrix/transaction-cells/`.

| Image | Native regression scripts | Result |
|---|---|---|
| debian-12 | transactions.sh, privilege.sh | pass |
| ubuntu-24.04 | transactions.sh, privilege.sh | pass |
| arch | pacman-transactions.sh | pass |
| fedora-previous | dnf-transactions.sh | pass |
| fedora-latest | dnf-transactions.sh | pass |
| debian-10 | transactions.sh, privilege.sh | pass |
| debian-11 | transactions.sh, privilege.sh | pass |
| ubuntu-18.04 | transactions.sh, privilege.sh | pass |
| ubuntu-20.04 | transactions.sh, privilege.sh | pass |
| ubuntu-22.04 | transactions.sh, privilege.sh | pass |

This covers APT 1.6 (Ubuntu 18.04), 1.8 (Debian 10), 2.0 (Ubuntu 20.04),
2.2 (Debian 11), 2.4 (Ubuntu 22.04), 2.6 (Debian 12) and 2.8 (Ubuntu 24.04)
for these scenarios. Newer Debian 13/sid APT consultation is checked by the
inspection matrix; their transactions are not separately run here. Fedora 43/44
exercise DNF5 and Arch exercises Pacman. This does not expand DNF execution
to DNF4.

### apk and Portage checkpoint — 2026-10-19

The new providers ran in fresh disposable VMs with the installed static musl
archive. Inspection (as the ordinary guest user): xpac's inventory equals an
independent native witness -- apk's own installed database
(`/lib/apk/db/installed`) for 200 packages, and `portageq match` split by the
specification's version syntax in Python for 418 -- then info, why, search and
export → diff → plan with no operation. Transactions (as guest root):
`tests/containers/apk-transactions.sh` and `tests/containers/portage-transactions.sh`.

| Image | Native version | Inspection | Transactions |
|---|---|---|---|
| alpine (3.24.1) | apk-tools 3.0.6 | pass | pass |
| gentoo | Portage 3.0.82.2 | pass | pass |

apk-tools 2.14 (Alpine 3.22) output, flags and WORLD behaviour were checked in a
container; no VM image of it is in the matrix. Installing Omniscient's
measurement witnesses through xpac then found two defects these scenarios did
not: apk right-aligns its transaction counter from ten operations on
(`( 1/10)`), and emerge rebuilds a dependency at its installed version (`R`,
bpftrace's zlib); both are fixed and covered by tests. Two QEMU processes of the
Gentoo cell were killed by an outside SIGKILL on a loaded host; the third run,
traced, received none and passed.

### Open: bpftrace's plan goes stale on Gentoo

Omniscient's cross-tool chain (`distroproof-devutils.toml`, target
`chain-gentoo`, Portage 3.0.82.2, kernel 6.18.57) asks xpac to install
`dev-debug/bpftrace` from the Gentoo binhost. On 2026-10-19 and twice on
2026-10-21 the apply was refused with `plan is stale; inspect a new plan
before applying`, after the same warnings the plan had printed (zlib, libffi,
zstd and libxml2 reinstalled at the versions they have). `apply -f` plans,
then `apply_native_guarded` plans again and requires the two to be equal: for
this request two successive `emerge --pretend` runs, a few seconds apart, do
not produce the same plan. The other eleven witnesses install, and an earlier
run of the same cell installed bpftrace. Which field differs is not known yet:
the next step is to keep both plans when they differ, in the refusal or in
the journal, and run the cell again.
