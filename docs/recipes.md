# Verified recipe preparation

`pkgx recipe check -f recipe.toml` validates and displays a recipe without downloading.
`pkgx recipe verify -f recipe.toml` downloads into private temporary storage, verifies
SHA-256 and discards the artifact. Neither command executes a script or establishes
ownership. Script installation, receipts and automated uninstall remain unsupported.

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
The original script is not piped, made executable, persisted or invoked. Failed
checks discard staging data. There is no integrity override. Tests use a mocked
downloader/checksummer and never run downloaded code or make network requests.
