# Security policy

## Supported versions

Security fixes are provided for the latest released minor version. During the
alpha period, only the newest release is supported.

## Reporting a vulnerability

Use GitHub private vulnerability reporting for the Verbsmith repository. Do not
open a public issue containing credentials, exploit details, or affected URLs.

Include the affected version, platform, reproduction steps, impact, and any
suggested mitigation. We aim to acknowledge reports within three working days
and will coordinate disclosure after a fix is available.

## Security properties

- No telemetry or update checks are performed automatically.
- Request files are untrusted input and must never execute native code.
- Secrets must be redacted from logs and reports.
- Sync servers accept encrypted workspace payloads and must reject stale writes.

Secrets are stored by the operating-system credential manager; the local index
contains names only. Sandboxed scripting remains a release blocker and will not
be described as complete until its threat model and audit are published.
