# Verbsmith

**A local-first API workspace for the terminal.**

Verbsmith combines a fast command-line client, an interactive terminal UI, and
Git-readable request collections. It works without an account, keeps requests
on your machine, and uses libcurl for mature network behavior.

> Status: early alpha. The workspace format is versioned, but compatibility is
> not guaranteed until 1.0. Do not use the plaintext `[secrets]` migration
> section for committed credentials.

## Why Verbsmith?

- Plain `.http` requests that produce useful Git diffs.
- One-off requests, reusable workflows, assertions, captures, and CI reports.
- No mandatory account, cloud sync, background analytics, or proprietary file format.
- A terminal interface for exploration and a deterministic CLI for automation.
- Explicit import failures instead of silently dropping unsupported options.
- Native binaries for supported macOS, Linux, and Windows targets.
- Explicit, end-to-end encrypted synchronization with a self-hosted server.

Verbsmith does not replace curl's FTP, SMTP, SCP, or other general transfer
protocols. It concentrates on API development and testing.

## Install from source

Rust 1.88 or newer and a C toolchain are required during the alpha period.

```console
cargo install --path crates/verbsmith-cli
verbsmith doctor
```

## Quick start

```console
verbsmith init demo
cd demo
verbsmith request GET https://httpbin.org/json
verbsmith run health --env local
verbsmith test all --format junit > results.xml
verbsmith
```

A request file looks like this:

```http
# @name create-session
# @assert status == 200
# @assert json $.authenticated == true
# @capture token = jsonpath("$.token")
POST {{base_url}}/session
Content-Type: application/json

{"email":"{{email}}","password":"{{password}}"}

###

# @name profile
# @depends-on create-session
GET {{base_url}}/profile
Authorization: Bearer {{token}}
```

See [the format guide](docs/guide/workspaces.md), [platform policy](docs/platforms.md),
[roadmap](ROADMAP.md), [public launch checklist](docs/launch.md), and
[contribution guide](CONTRIBUTING.md).

## Privacy and security

Verbsmith sends network traffic only when you ask it to execute or synchronize.
Telemetry is disabled and no central Verbsmith account exists. Diagnostic logs
redact registered secrets. Please report vulnerabilities privately according to
[SECURITY.md](SECURITY.md).

## License

Licensed under either Apache-2.0 or MIT, at your option.

Verbsmith is independent and is not affiliated with the curl project, Postman,
Insomnia, or Bruno. Built by [Sreejith Muralidharan](https://sreejith.co.uk).
