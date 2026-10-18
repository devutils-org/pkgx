# ADR 008: Tracked script installation and receipts

Status: implemented experimentally; simulated-interpreter regressions and a real
install/uninstall in a loopback-only bubblewrap sandbox (docs/validation.md).

A recipe becomes executable only through `pkgx recipe install`, which requires:

- a valid strict recipe with pinned version, concrete SHA-256 and HTTPS source;
- an explicit `uninstall` action and at least one declared resource;
- home-relative `~/` resources only, below the invoking user's absolute HOME,
  with no symlinked or non-directory ancestor; system paths are never tracked;
- every declared resource absent before execution (checked at preview and again
  immediately before the script starts) so that nothing pre-existing is claimed;
- a regular, non-root user; and interactive `yes` or `--yes` trust confirmation.

Execution. The verified download is stored in a private content-addressed store and
re-hashed in-process before every run. It is passed by path to a fixed interpreter
(`/bin/sh`, `/bin/bash` or `/usr/bin/python3`) with the recipe's literal arguments
plus PKGX_RECIPE_NAME/PKGX_RECIPE_VERSION. No shell string, pipe or sudo is used. The
script runs with the user's full permissions; it is not confined.

Receipts. A private atomic receipt with status `install_started` is written before
execution. Afterwards each declared resource gets evidence: regular file
(SHA-256 + size) or symlink (target) means owned; anything else is `missing` or
`unsupported` and is never claimed. Status becomes `installed` only when the script
exits 0 and every resource is owned; otherwise `install_incomplete`. Recipe list
classifies receipts as `tracked` or `tracked_incomplete`; everything without a
receipt is unmanaged.

Uninstall. Interrupted receipts (`*_started`) are refused: ownership is unknown and
`recipe forget` only drops the receipt. Otherwise every owned file/link/service must
still match its evidence (configuration may be edited) and the stored artifact must
still match the digest, or nothing runs. The recipe's own uninstall action is the only
remover: pkgx never deletes resource paths itself. Afterwards owned file/link/service
resources must be gone; configuration may remain and is reported for manual review.
Success removes the receipt and its unreferenced artifact; otherwise the receipt
stays `uninstall_incomplete`.

Not implemented: experimental observation of undeclared resources (the roadmap's
`observed` class), system-scope recipes, confinement, and integrity overrides.
Undeclared side effects of a script are neither observed nor reversed.
