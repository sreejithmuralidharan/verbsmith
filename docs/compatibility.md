# Compatibility status

This page distinguishes implemented behavior from the 1.0 roadmap. It is
updated in the same pull request as each capability.

| Capability | Alpha status |
| --- | --- |
| HTTP/1.1 and HTTP/2 requests through bundled libcurl | Available |
| Variables, environments, dependencies, assertions, captures | Available |
| Pretty, raw, JSON, JSONL, JUnit, and SARIF output | Available |
| Interactive terminal request browser | Available, intentionally minimal |
| Operating-system credential vault | Available |
| curl command import | Common flags; unsupported options fail explicitly |
| Postman Collection 2.1 import | Requests, headers, raw bodies; loss report for auth/scripts |
| Versioned encrypted sync protocol | Push, pull, status, local backups, stale-write rejection, and age encryption available |
| Browser/WASM parser and formatter | Available |
| HTTP/3 | Build support under validation |
| GraphQL, WebSocket, SSE, and gRPC adapters | Planned for beta |
| Sandboxed scripts and WASI plugins | Planned for beta |
| Insomnia, Bruno, HAR, OpenAPI import/export | Planned for beta |
| Checksummed multi-platform packages with GitHub build attestations | Configured for tagged releases; first release pending |

The CLI reports the exact linked libcurl capabilities through `verbsmith doctor`.
The supported target tiers and verification rules are documented in
[platforms.md](platforms.md).
