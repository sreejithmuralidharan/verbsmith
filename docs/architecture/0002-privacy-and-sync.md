# ADR 0002: Local-first privacy and sync

Status: accepted

Local operation requires no account and performs no analytics, update check, or
background synchronization. Sync is explicit and points to a user-configured
open-source server.

The server stores immutable encrypted revisions and metadata. Clients perform
encryption and conflict resolution. A stale base revision returns HTTP 409;
clients must merge or ask the user rather than overwriting newer work.

