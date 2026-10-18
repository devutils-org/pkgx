# Script recipes

`pkgx recipe check -f recipe.toml` validates and displays a recipe without downloading.
`pkgx recipe verify -f recipe.toml` downloads into private temporary storage, verifies
SHA-256 and discards the artifact. Neither command executes a script.

`pkgx recipe install -f recipe.toml [--dry-run|--yes]` previews, then (after trust
confirmation) runs the verified script as your user and records a receipt.
`pkgx recipe uninstall NAME [--dry-run|--yes]` runs the recipe's recorded uninstall
action after ownership checks. `pkgx recipe list` shows receipts (`tracked` or
`tracked_incomplete`); anything else is unmanaged. `pkgx recipe forget NAME` drops a
receipt without running anything or touching files. Receipts live under the state
directory (`--state-dir`, default `$XDG_STATE_HOME/pkgx/transactions/scripts`).
See [ADR 008](adr/008-script-receipts.md) for the ownership rules.

An installable recipe declares home-relative resources:

```toml
version = 1
name = "demo-tool"
pinned_version = "1.0.0"
url = "https://example.org/releases/1.0.0/install.sh"
sha256 = "<replace-with-a-verified-64-digit-digest>"
interpreter = "sh"
install = { args = ["install"] }
uninstall = { args = ["uninstall"] }
resources = [
  { kind = "file", path = "~/.local/bin/demo-tool" },
  { kind = "configuration", path = "~/.config/demo-tool.toml" },
]
```

Absolute declarations such as the one below remain valid for check/verify but are
not installable: system paths are never tracked or claimed.

```toml
version = 1
name = "example-tool"
pinned_version = "1.2.3"
url = "https://example.org/releases/1.2.3/install.sh"
sha256 = "<replace-with-a-verified-64-digit-digest>"
interpreter = "sh"
[install]
args = ["--prefix", "/opt/example-tool"]
[uninstall]
args = ["--uninstall", "--prefix", "/opt/example-tool"]
[[resources]]
kind = "file"
path = "/opt/example-tool/bin/example-tool"
```

The placeholder is deliberately invalid. A pinned version, concrete SHA-256 digest,
HTTPS source without credentials/query/fragment, interpreter enum and literal argument
arrays are required. Interpreters are sh/bash/python3; no free-form command strings
are accepted. Actions are metadata only. Resource kinds are file/link/configuration/
service, with unique absolute non-root paths and no parent traversal. Declarations
do not prove that resources were created by the recipe or are safe to remove.

Verification uses `curl` with curlrc disabled, HTTPS-only transport, redirects refused,
an 8 MiB size limit enforced by both curl and bounded stdout capture and bounded time. Linux uses `sha256sum`, macOS `shasum -a 256`.
Verify never pipes, persists or invokes the artifact. Install stores it privately,
re-hashes it before each run and passes it by path to a fixed interpreter; failed
checks discard staging data. There is no integrity override. Unit tests use a mocked
downloader/checksummer/interpreter. The real lifecycle check
(tests/sandbox/recipe-userns.sh) runs only its own fixture script in a read-only,
loopback-only sandbox with a private HOME.
