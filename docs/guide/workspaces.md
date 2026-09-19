# Workspace format

`verbsmith.toml` identifies the workspace and its schema version. Requests live
under `requests/` as UTF-8 `.http` files; environments live under
`environments/` as TOML.

## Directives

Directives are comments placed before a request:

| Directive | Meaning |
| --- | --- |
| `# @name login` | Stable request identity; required for dependencies. |
| `# @tag smoke, auth` | Searchable collection tags. |
| `# @depends-on login` | Execute named dependencies first. |
| `# @timeout 1500ms` | Override the request timeout. |
| `# @disabled` | Keep the request without executing it. |
| `# @assert status == 200` | Require an exact HTTP status. |
| `# @assert header content-type contains "json"` | Inspect a response header. |
| `# @assert body contains "ready"` | Search the decoded response bytes. |
| `# @assert json $.user.id == 42` | Compare a JSON path. |
| `# @capture token = jsonpath("$.token")` | Save a response value for later requests. |
| `# @capture etag = header("etag")` | Save a response header. |

Multiple requests may share a file when separated by `###`. Variable templates
use `{{name}}`. Resolution order is command-line overrides, request-local values,
data-row values, the selected environment, workspace values, then built-ins.
Unresolved variables are errors.

## Compatibility guarantees

Schema upgrades must be deterministic and backed up before modification. Parsers
must preserve unknown extension fields. Importers must report unsupported input
rather than discarding it. The schema remains provisional before version 1.0.

