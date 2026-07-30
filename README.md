# ACT MCP server

Hardened Streamable HTTP Model Context Protocol service for the AntiCapTrad
platform. The current tool surface is intentionally tiny and read-only; it is a
secure foundation for future organization-specific diagnostics rather than a
public general-purpose execution endpoint.

## HTTP surface

| Route | Access | Purpose |
| --- | --- | --- |
| `GET /health` | public | kubelet liveness probe |
| `GET /ready` | public | kubelet readiness probe |
| `POST /mcp` | `x-server-auth` required | MCP JSON-RPC requests and notifications |

The MCP endpoint implements `initialize`, `ping`, `tools/list`, and `tools/call`.
It accepts protocol revisions `2025-03-26`, `2025-06-18`, and `2025-11-25`, and
advertises `2025-11-25` when the client does not propose a supported revision.

## Security model

- `SERVER_AUTH_SECRET` must contain at least 24 bytes. If it is absent, `/mcp`
  fails closed with HTTP 503.
- Native MCP clients may omit `Origin`. Browser requests must send an exact
  normalized origin listed in comma-separated `MCP_ALLOWED_ORIGINS`; `*` is
  rejected.
- Request bodies are capped at 64 KiB.
- JSON-RPC request IDs must be strings or integers. Explicit `null`, fractional,
  boolean, object, and array IDs are rejected.
- Every ID-less notification receives HTTP 202 with an empty body.
- Tool definitions carry read-only, non-destructive, idempotent, closed-world
  annotations. The current `ping` argument is bounded to 1,024 bytes and rejects
  control characters.
- Authentication secrets are compared in constant time after fixed-width hashing
  and are never included in responses or logs.

## Configuration

```text
PORT=8080
OTEL_SERVICE_NAME=act-mcp-server
SERVER_AUTH_SECRET=<at-least-24-random-bytes>
MCP_ALLOWED_ORIGINS=https://console.example,http://localhost:3000
```

No dotenv loader is used. Supply configuration through the process environment
or Kubernetes Secret/ConfigMap projections.

## Run locally

```sh
export SERVER_AUTH_SECRET='replace-with-at-least-24-random-bytes'
cargo run -- --port=8080
```

Initialize the server:

```sh
curl -sS http://127.0.0.1:8080/mcp \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -H 'mcp-protocol-version: 2025-11-25' \
  -H "x-server-auth: $SERVER_AUTH_SECRET" \
  -d '{"jsonrpc":"2.0","id":"init","method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"curl","version":"1"}}}'
```

List tools:

```sh
curl -sS http://127.0.0.1:8080/mcp \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -H 'mcp-protocol-version: 2025-11-25' \
  -H "x-server-auth: $SERVER_AUTH_SECRET" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'
```

## Checks

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo build --locked --release
cargo audit --deny warnings
```

See [`TESTING.md`](TESTING.md) for the adversarial protocol matrix.
