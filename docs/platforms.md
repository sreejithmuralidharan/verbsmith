# Platform support

Verbsmith uses support tiers so that “cross-platform” has a testable meaning.
The project does not claim to run on every operating system or architecture.

## Tier 1: release binaries and continuous integration

- Linux x86-64 and ARM64
- macOS x86-64 and Apple silicon
- Windows x86-64 and ARM64

Every tagged release builds both `verbsmith` and `verbsmith-server`, runs the
workspace test suite, publishes SHA-256 checksums, and records a GitHub build
provenance attestation. A target is not considered supported when its CI job is
disabled or failing.

## Tier 2: source support

FreeBSD, OpenBSD, other Linux architectures, and terminal environments such as
Termux are welcome when Rust, a C toolchain, and the native credential-store
dependencies are available. These targets are best effort until a maintained CI
runner and a release owner exist.

## Portability rules

- Request workspaces use UTF-8 text and `/` as the serialized path separator.
- Tests must not assume a Unix shell, `/tmp`, case-sensitive paths, or POSIX-only
  filesystem replacement behavior.
- Platform-specific credential stores are accessed through the `keyring` crate.
- Linux source builds require Kerberos/GSSAPI development headers for SPNEGO
  authentication (`libkrb5-dev` on Debian and Ubuntu).
- Features that depend on a transport capability must check the bundled libcurl
  build and appear in `verbsmith doctor`.
- A platform regression blocks a release for its support tier and must not be
  hidden by relabeling the job as optional.

Support reports should include `verbsmith --version`, `verbsmith doctor`, the
operating-system version, architecture, terminal, and a minimal reproduction.
