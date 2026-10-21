# Security policy

xpac runs package manager transactions and tracked install scripts, so security
reports matter.

## Reporting a vulnerability

Please do not open a public issue. Use GitHub's
[private vulnerability reporting](https://github.com/devutils-org/xpac/security/advisories/new)
with what you ran, what happened, and the xpac version (`xpac --version`).
You will get an answer within a week.

## Supported versions

Only the latest release gets fixes. xpac is at 0.1: native execution is
experimental and not certified (see [support](docs/support.md)).

## Verifying a release

Each release archive carries a build provenance attestation:

```sh
gh attestation verify xpac-v0.1.0-TARGET.tar.gz --repo devutils-org/xpac
```

The boundaries xpac keeps (no implicit elevation, confirmed plans, confined
recipe paths) and their review are in [docs/security-review.md](docs/security-review.md).
