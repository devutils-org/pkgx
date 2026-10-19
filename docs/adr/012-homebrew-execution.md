# ADR 012: Homebrew formula preview and execution

Status: implemented experimentally; injected-runner regressions and Homebrew 7.0.9
on Linux in a disposable rootless Arch root (docs/validation.md). macOS remains
unvalidated.

Scope: formulae only. Casks are refused for execution (their app bundles and
uninstall stanzas are outside this contract). Homebrew is never run as root or
through sudo; `--sudo` is refused for Homebrew.

Every xpac-built brew command sets HOMEBREW_NO_AUTO_UPDATE, NO_ANALYTICS,
NO_INSTALL_CLEANUP, NO_ENV_HINTS, NO_COLOR and NO_AUTOREMOVE. The last one matters:
since Homebrew 4.3, `brew uninstall` autoremoves unused dependencies, which the
native validation caught as an unreviewed effect.

Preview. `brew install --dry-run --formula NAME...` lists formulae and dependencies
to install; any other "Would ..." action fails closed. Recent Homebrew omits
dependency versions; they are completed from `brew info --json=v2` (stable plus
revision). The preview is uncertain: post-install steps, linking and source builds
are not modeled. Removal requires exactly one installed keg and no installed
dependents (`brew uses --installed`).

Execution repeats the preview, refuses a stale plan and runs
`brew install --formula NAME...` or `brew uninstall --formula NAME...` with no
force or ignore-dependency options. Versions cannot be pinned; each installed keg
version is verified against the preview and untouched formulae must not change.

Homebrew 7 receipts carry only `installed_on_request`; it alone now determines the
native install reason when `installed_as_dependency` is absent.
