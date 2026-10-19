# ADR 015: Portage preview and execution

Status: implemented experimentally; injected-runner regressions
(`crates/xpac-core/tests/portage_exec.rs`) and disposable Gentoo guests (Portage
3.0.82) in Distroproof. Portage before 3.0 is refused.

Inventory. The installed database, `/var/db/pkg/CATEGORY/NAME-VERSION/`, is read
with one `grep -r --include=SLOT --include=repository` over it: no Python start
per package. The directory is split at the first hyphen whose remainder is a
version by the package manager specification. An identity is
`portage:category/name:slot`; the subslot and the repository are metadata. A
package is explicit when the world set (`/var/lib/portage/world`) selects it,
for its slot if the atom names one; Portage records selections, not reasons, so
anything else is `unknown` -- a dependency, or a member of the system set.

Preview. `emerge --pretend --noreplace --autounmask=n --quiet` resolves without
root and writes nothing. Each `[ebuild|binary FLAGS] category/name-version` row is
an install: new (`N`) or an upgrade (`U`, flagged as a partial-upgrade risk). A
binary's trailing `-BUILD_ID` is no part of its version. A rebuild (`R`) is accepted
only at the version installed -- a dependency rebuilt with other flags or for a
subslot, as bpftrace's zlib on Gentoo -- and flagged; downgrades (`D`), blockers and
any `[blocks]`/`[uninstall]` row are refused, as is
anything masked (emerge exits 1). Rows built from source are flagged. A request
already installed but not selected is a `MarkExplicit` operation: emerge records it
in world without rebuilding. Binary packages are used only when the user's own
configuration enables them (`FEATURES=getbinpkg`); xpac never adds it.

Removal is `emerge --depclean =category/name-version` of exactly the installed
version: Portage removes it only when nothing installed needs it, and deselects it
from world (deselection is implied by every uninstall action). The preview runs
the same `--pretend --depclean` and requires the selected set to be exactly the
request; a protected package is refused, naming it. `--unmerge`, which removes
what installed packages need, is never used. Dependencies stay installed.

Execution repeats the preview, refuses a stale plan, then runs `emerge --ask=n
--noreplace --autounmask=n --nospinner --color=n --quiet -- =category/name-version...`
pinned to the reviewed versions (a selection-only request by name), never with
`--update`, `--deep`, `--newuse`, `--oneshot` or `--sync`, and verifies every
reviewed version, selection and the untouched inventory afterwards. World records
the request without its version, so no pin outlives the transaction.

Privilege. `--sudo` delegates only those two templates followed by non-option
targets to `/usr/bin/emerge`, which must be the binary selected on PATH, and
refuses `PORTAGE_CONFIGROOT`, `ROOT` and `EROOT` overrides: inspection and
execution must read the same database.
