# xpac documentation

Start with the [README](../README.md), then the [CLI reference](../REFERENCE.md).

## Using xpac

- [Install, verify and uninstall](install.md): release archives, attestations, completions
- [Manifests](manifest.md): the TOML profile `plan` and `apply` converge to
- [Backends](backends.md): what each package manager supports, and how far
- [Script recipes](recipes.md): tracked installs outside a package manager
- [Failure and recovery](recovery.md): journals, partial transactions, what to do next
- [Testing guide](how-to-test.md) (French): every feature by hand, safest first

## Evidence

- [Support status](support.md): each platform, its evidence and its CI
- [Validation checkpoint](validation.md): exact commands, counts, and what was not run
- [Remaining manual validation](manual-validation.md) (French)
- [Linux portability with distroproof](distroproof.md): the installed-release matrix
- [Security review](security-review.md): privilege boundaries, scripts, cleanup

## Design

- [Roadmap](ROADMAP.md): the product contract, milestones M0–M8
- [Decisions](adr/): one ADR per design choice, 001 to 015

Maintainers' working plans and notes are in [dev/](dev/); they are not user
documentation.
