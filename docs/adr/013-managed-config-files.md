# ADR 013: Managed user configuration files

Status: implemented experimentally except removal. Schema, layering, confined
sources, `diff` states, CLI `plan -f`/`apply -f` writes and `pkgx file adopt|list|
forget` have unit and CLI regressions against temporary HOME directories; no
sandbox validation yet. `pkgx file remove` is not implemented. The core/TUI package
planners still refuse manifests declaring files. Roadmap milestone M8.

Users want their configuration (e.g. `~/.tmux.conf`) declared next to the packages
that read it. pkgx manages such files as opaque owned content: it copies declared
bytes and tracks what it wrote. It never parses, merges, templates or generates
application configuration, and it never runs reload actions.

Schema. Manifest v1 gains an optional `files` table keyed by target, available in the
base, platform and profile layers:

```toml
[files."~/.tmux.conf"]
source = "dotfiles/tmux.conf"
mode = "0644"

[platforms.macos.files."~/.config/alacritty/alacritty.toml"]
source = "dotfiles/alacritty-macos.toml"

[profiles.dev.files."~/.config/nvim/init.lua"]
source = "dotfiles/nvim/init.lua"
```

Each declaration accepts only `source` (required) and `mode` (optional, default
`0600`; allowed `0600`, `0644`, `0700`, `0755`). Unknown fields, inline content,
variables, globs, directories and interpolation are rejected. This stays
`version = 1`: every existing manifest remains valid with identical meaning, and an
older pkgx rejects the unknown `files` field instead of silently ignoring it.

Targets must start with `~/`, are normalized lexically (no `.`/`..` components, empty
components or trailing slash) and must resolve below the invoking user's absolute
HOME. System paths are out of scope. Layering follows ADR 003: identical declarations
of one target across active layers collapse; differing declarations are an error,
since layers never override. A duplicate key within one layer is a TOML error.

Sources are relative paths resolved against the manifest's directory. Every component
is opened without following symlinks, the final entry must be a regular file inside
that directory, and each source is limited to 1 MiB (16 MiB per manifest). Sources are
read once; their SHA-256 binds the reviewed plan and is checked again before writing.

Deployment is copy, not symlink. A symlink into a checkout makes unreviewed edits live
immediately, breaks when the checkout moves and is exactly the destination shape the
writer refuses to follow. Copies make drift measurable by digest.

Observation (read-only, creates no state). Each target is classified as:

- `missing`: absent;
- `in_sync`: managed, content and mode equal to the declaration (also when a local
  edit was copied back into the source: nothing would be overwritten);
- `outdated`: managed, still matching its receipt, but the declaration changed;
- `drifted`: managed, content or mode differs from both the receipt and the
  declaration;
- `unmanaged_conflict`: present without a receipt (whatever its content);
- `unsupported`: symlink, directory, special file, foreign owner, or a symlinked or
  non-directory ancestor;
- `undeclared`: a receipt whose target is no longer declared (retained, never removed).

`diff` reports these states. `plan -f` proposes writes only for `missing` and
`outdated`; `drifted`, `unmanaged_conflict` and `unsupported` make the plan fail with
the target named, after package planning so all problems are reported together. An
`unmanaged_conflict` whose content already equals the source is still refused: equal
bytes do not prove the user delegated the file. Previews print a unified diff for
UTF-8 text (digest and size otherwise) on stderr, prefixed with a warning that file
contents are displayed.

Receipts and adoption. A private versioned registry under `<state-dir>/files/` (same
locking, atomic write, fsync and fail-closed rules as ADR 005) stores per target: the
written SHA-256, size and mode, the source digest and the manifest path. Ownership is
only ever granted by a pkgx write or by an explicit adoption:

```sh
pkgx file adopt ~/.tmux.conf --dry-run
pkgx file adopt ~/.tmux.conf
```

Adoption previews the current content, copies it to a private content-addressed backup
(`<state-dir>/files/backups/<sha256>`, mode 0600, never deleted automatically), then
records the current digest as the receipt. The next `apply` therefore sees the file as
`outdated` and replaces it after the usual review. Re-adopting a `drifted` file is how
local edits are discarded deliberately; the edited bytes are backed up first.
`pkgx file list` shows receipts; `pkgx file forget TARGET` drops a receipt and touches
nothing on disk.

Writing. `apply -f` plans files before any package transaction (conflicts abort
early), then runs package transactions, then shows the file preview and asks for its
own confirmation; a package failure stops before any file is written. Files need no
privileges: writes as root are refused, and `--sudo` never applies to them. For each
target, immediately before writing, the registry snapshot, the target's planned state
(absent, or digest and mode) and the re-read source digest must all still match the
review. Missing parent directories are created with mode 0700 without following
symlinks. Content goes to an exclusively created `.pkgx-*` temporary file in the same
directory, gets its mode, is fsynced, renamed over the target (no-clobber for creates;
a rename replaces a racing symlink itself rather than following it) and the directory
is fsynced; then the receipt is updated and the result verified. The first failure
stops later files. File writes are not in the native transaction journal; receipts are
their record. A crash after the rename but before the receipt leaves a file reported as
`in_sync` (updates) or `unmanaged_conflict` (creates, adopt to resume); a crash before
the rename may leave a `.pkgx-*` temporary file. A concurrent process can still change
a target between the final check and the rename; this residual race is documented,
not hidden.

Removal. Dropping a declaration only reports the target as `undeclared` and keeps its
receipt; nothing is deleted. A future `pkgx file remove TARGET [--dry-run|--yes]` (not implemented) deletes a
file only when it still matches its receipt, after preview and confirmation, then
drops the receipt. Drifted, unmanaged or unsupported files are never deleted. Strict
mode (ADR 005) does not extend to files.

Not covered: system paths and services, reload/restart hooks (tmux `source-file` may
be suggested, never run), templating and per-host variables, secrets (contents are
copied and shown verbatim; storing secrets in manifests is discouraged), directory
trees, partial edits of foreign files and symlink deployment. `export` never captures
file contents. These require a new ADR.

Validation plan: unit tests with an injected filesystem root, then a loopback-only
sandbox with a private HOME covering every state above, adoption backups, interrupted
writes, symlink attacks on targets/sources/ancestors and repeated apply as a no-op.
