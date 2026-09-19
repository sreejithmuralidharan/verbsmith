# Contributing to Verbsmith

Thank you for improving Verbsmith. Discussions are welcome before substantial
changes; small bug fixes can go directly to a pull request.

## Development

1. Install the Rust toolchain selected by `rust-toolchain.toml`.
2. Run `cargo test --workspace`.
3. Run `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings`.
4. Add tests and user-facing documentation for behavioral changes.
5. Add a signed-off-by line to each commit (`git commit -s`) to certify the DCO.

Contributions are accepted under `MIT OR Apache-2.0`. Contributors are
responsible for understanding, testing, and having the right to submit every
line they contribute, regardless of which development tools they use.

Changes must not silently discard imported fields, weaken secret redaction, add
telemetry, or introduce an unexpected network call. Format changes require a
schema migration, fixture, and architecture decision record.

## Pull requests

- Keep commits focused and explain the user problem.
- Include exact verification commands and results.
- Avoid drive-by dependency additions and generated files that cannot be reproduced.
- Update `CHANGELOG.md` under `Unreleased` for user-visible changes.

By contributing, you certify the [Developer Certificate of Origin](https://developercertificate.org/).

