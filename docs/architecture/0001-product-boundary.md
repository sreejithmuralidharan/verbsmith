# ADR 0001: Product boundary and transport

Status: accepted

Verbsmith is an API workspace, not a reimplementation of every curl protocol.
It targets HTTP/1.1–3, REST, GraphQL, WebSocket, SSE, and gRPC.

Rust owns the workspace, orchestration, security boundaries, TUI, and reporting.
A pinned bundled libcurl build provides mature HTTP, TLS, proxy, and
authentication behavior. Dedicated Rust adapters provide protocol behavior that
libcurl does not model, notably gRPC. The transport interface remains internal so
implementations can be tested against common conformance fixtures.

