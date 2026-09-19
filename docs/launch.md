# Public launch checklist

GitHub stars are an outcome of a useful, trustworthy project. Verbsmith will
not buy stars, automate follows, or post unsolicited promotions.

## Before publishing

- Complete the trademark searches in [name-clearance.md](name-clearance.md).
- Reserve the repository, crate, domain, and social names together.
- Protect `main`, require CI, signed commits, and review for workflow changes.
- Enable private vulnerability reporting, Discussions, Dependabot alerts, and
  secret scanning.
- Add repository topics: `api-client`, `cli`, `http`, `rust`, `terminal`,
  `postman-alternative`, `local-first`, and `developer-tools`.
- Verify a clean installation on every Tier 1 target from the packaged archive,
  not from a development checkout.

## First release

- Publish `verbsmith-core` before the CLI and WASM crates that depend on it.
- Tag the exact reviewed commit and let the release workflow build artifacts.
- Verify every SHA-256 checksum and GitHub provenance attestation.
- Include a two-minute terminal recording showing init, import, assertions,
  CI output, vault redaction, and encrypted sync.
- Publish a candid compatibility table and known limitations with the release.
- Announce to relevant communities only where project sharing is permitted,
  answer technical questions, and turn repeated feedback into public issues.

## Ongoing

- Ship small, reviewable releases with useful notes and migration guidance.
- Keep good-first-issue tasks genuinely bounded and mentor contributors.
- Publish performance and compatibility measurements with reproducible scripts.
- Respond to confirmed security reports and data-loss bugs before feature work.
- Recheck comparison claims whenever competitors change their products.
