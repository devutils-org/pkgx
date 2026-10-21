# Contributing to xpac

Issues and pull requests are welcome at
[devutils-org/xpac](https://github.com/devutils-org/xpac).

## Before a pull request

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 scripts/test_package.py
```

## Rules the code keeps

- **Never touch the host in tests.** Native transactions, recipe scripts and cleanup
  run only in disposable roots (`tests/sandbox/`) or CI containers. Unit tests use
  fixtures and injected runners.
- **Plan first.** Every mutation goes through a plan the user can read, with
  explicit confirmation. A provider that cannot preview natively stays unsupported
  rather than guessing.
- **Unsupported is an answer.** Never substitute a feature silently; say what is
  missing.
- **A design choice gets an ADR** in [docs/adr/](docs/adr/), numbered after the last one.
- **Evidence is recorded**: a new native check goes into
  [docs/validation.md](docs/validation.md) and [docs/support.md](docs/support.md)
  with the run that passed.

See the [documentation index](docs/README.md) for the rest.

## License

Contributions are dual-licensed under [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE), at the user's option, unless stated otherwise.
