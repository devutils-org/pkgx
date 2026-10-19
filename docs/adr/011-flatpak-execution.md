# ADR 011: Flatpak preview and execution

Status: implemented experimentally; injected-runner regressions and a disposable
rootless Arch root with Flatpak 1.18.4 and a locally built repository
(docs/validation.md). No public remote was contacted.

Flatpak has no dry run. The preview is therefore explicitly uncertain and built from
native queries:

- the ref must be offered by exactly one configured remote of the requested scope
  (`flatpak remotes`, `flatpak remote-info --show-commit`); zero or several remotes
  are refused, because xpac never chooses an origin;
- the reviewed commit is the remote's full commit digest;
- an app's runtime (`--show-runtime`) is added when installed in neither scope and
  must be offered by the same remote, otherwise the user installs it explicitly;
- already-installed refs are a verified no-op;
- removal is limited to apps and never removes their runtime; runtime removal is
  refused because other apps may depend on it.

Execution repeats the preview, refuses a stale plan and runs
`flatpak install --<scope> --noninteractive --no-related -- REMOTE REF...` or
`flatpak uninstall --<scope> --noninteractive -- REF...`. `--no-related` keeps the
transaction to the reviewed refs: locale and extension refs are not installed
implicitly. Afterwards each installed ref's origin must equal the reviewed remote and
its active commit must match the reviewed digest; untouched refs must not change.

Privilege. xpac never wraps flatpak in sudo: system-scope changes go through
Flatpak's own system helper and polkit, and `--sudo` is refused for Flatpak.
