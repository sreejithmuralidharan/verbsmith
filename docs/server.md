# Self-hosted sync server

The alpha server exposes an authenticated, revision-safe storage protocol for
encrypted workspace snapshots. It never accepts an implicit overwrite.

```console
export VERBSMITH_SERVER_TOKEN="replace-with-at-least-24-random-characters"
cargo run -p verbsmith-server
```

Configuration:

| Variable | Default | Meaning |
| --- | --- | --- |
| `VERBSMITH_SERVER_ADDR` | `127.0.0.1:8787` | Listen address. |
| `VERBSMITH_SERVER_DATABASE` | `verbsmith-server.sqlite3` | SQLite database. |
| `VERBSMITH_SERVER_TOKEN` | required | Bootstrap bearer token. |

API:

- `GET /health`
- `GET /api/v1/workspaces/{uuid}/head`
- `PUT /api/v1/workspaces/{uuid}/revisions`

Revision requests contain `base_revision` and base64-encoded age `ciphertext`
payload. A stale base returns `409 Conflict`. The server intentionally does not
encrypt plaintext on behalf of clients.

Configure a workspace with a stable UUID:

```toml
[sync]
endpoint = "https://sync.example.com"
workspace_id = "123e4567-e89b-12d3-a456-426614174000"
token_secret = "sync-token"
encryption_secret = "sync-key"
```

Store both values in the operating-system credential manager, then synchronize:

```console
verbsmith vault set sync-token
verbsmith vault set sync-key
verbsmith sync status
verbsmith sync push
verbsmith sync pull
```

Snapshots are encrypted client-side using the age passphrase format. Pulls reject
locally modified workspaces unless `--force` is supplied, and overwritten files
are copied to `.verbsmith/backups/` first. This alpha implementation still
requires independent security review before production secret synchronization.
