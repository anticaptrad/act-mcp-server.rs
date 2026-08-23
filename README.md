# ACT MCP server

Hardened HTTP Model Context Protocol compatibility service for the AntiCapTrad
platform. The current tool surface is intentionally tiny and read-only; it is a
secure foundation for future organization-specific diagnostics rather than a
public general-purpose execution endpoint.

The repository currently carries a reviewed custom `2025-11-25` JSON-response
compatibility endpoint. Migration to official `rmcp` 3.0.1 is intentionally a
separate lifecycle change because the 2026 protocol candidate changes session
and initialization behavior. The custom endpoint is therefore guarded by
explicit CI ratchets and real-process conformance tests rather than treated as a
permanent undocumented exception.

## HTTP surface

| Route | Access | Purpose |
| --- | --- | --- |
| `GET /health` | public | kubelet liveness probe |
| `GET /ready` | public | kubelet readiness probe |
| `POST /mcp` | exact Host + `x-server-auth`; allowlisted Origin for browsers | MCP JSON-RPC requests and notifications |

The MCP endpoint implements `initialize`, `ping`, `tools/list`, and `tools/call`.
It accepts protocol revisions `2025-03-26`, `2025-06-18`, and `2025-11-25`, and
advertises `2025-11-25` when the client does not propose a supported revision.

## Security model

- `SERVER_AUTH_SECRET` must be 24 bytes through 4 KiB and contain no whitespace
  or control characters. If absent, `/mcp` fails closed with HTTP 503.
- Every HTTP MCP request must use an exact normalized authority listed in
  `MCP_ALLOWED_HOSTS`. Missing or hostname-lookalike values receive HTTP 421.
- Native MCP clients may omit `Origin`. Browser requests must send an exact
  normalized origin listed in `MCP_ALLOWED_ORIGINS`; `*` is rejected.
- Request bodies are capped at 64 KiB.
- Protected and public responses include `Cache-Control: no-store`,
  `X-Content-Type-Options: nosniff`, `Referrer-Policy: no-referrer`, and a
  closed-world Content Security Policy.
- JSON-RPC envelopes reject unknown top-level fields. Request IDs must be
  strings or integers; explicit `null`, fractional, boolean, object, and array
  IDs are rejected.
- Every ID-less notification receives HTTP 202 with an empty body.
- `tools/call` rejects unknown envelope fields; the `ping` runtime validator
  enforces the same closed schema advertised to clients.
- Tool definitions carry read-only, non-destructive, idempotent, closed-world
  annotations. The current `ping` argument is bounded to 1,024 bytes and rejects
  non-string values, unknown fields, and control characters.
- Unknown tool names are not reflected in errors.
- Authentication secrets are compared in constant time after fixed-width hashing
  and are never included in responses, logs, or traces.

## Configuration

```text
PORT=8080
OTEL_SERVICE_NAME=act-mcp-server
SERVER_AUTH_SECRET=<24-to-4096-byte-whitespace-free-random-value>
MCP_ALLOWED_HOSTS=localhost,localhost:8080,127.0.0.1,127.0.0.1:8080,[::1],[::1]:8080,act-mcp-server,act-mcp-server:8080
MCP_ALLOWED_ORIGINS=https://console.example,http://localhost:3000
```

No dotenv loader is used. Supply configuration through the process environment
or Kubernetes Secret/ConfigMap projections. `SERVER_AUTH_SECRET` remains
excluded from the CLI contract; Host, Origin, port, and log-filter settings are
non-secret `flags-2-env` options.

## Run locally

```sh
export SERVER_AUTH_SECRET='replace-with-at-least-24-random-bytes'
cargo run -- --port=8080 --allowed-hosts=127.0.0.1:8080,localhost:8080
```

Initialize the server:

```sh
curl -sS http://127.0.0.1:8080/mcp \
  -H 'host: 127.0.0.1:8080' \
  -H 'content-type: application/json' \
  -H 'accept: application/json' \
  -H 'mcp-protocol-version: 2025-11-25' \
  -H "x-server-auth: $SERVER_AUTH_SECRET" \
  -d '{"jsonrpc":"2.0","id":"init","method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"curl","version":"1"}}}'
```

## Tests

The unit/router tests cover exact Host and Origin policy, constant-time secret
gating, security headers, request-size limits, lifecycle behavior, IDs, runtime
tool schemas, bounded ping messages, and generic errors.

`tests/http_protocol.rs` launches the real binary and proves over a TCP socket:

- stable protocol initialization;
- exact Host enforcement and hostname-lookalike rejection;
- missing-secret rejection;
- security headers on real responses;
- unknown tool arguments and top-level envelope fields fail closed;
- the synthetic secret never appears on stderr.

## Checks

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps
cargo build --locked --release
cargo audit --deny warnings
```

See [`TESTING.md`](TESTING.md) for the broader adversarial protocol matrix.

## Environment secrets

Secrets live in this repo **encrypted** with [sops](https://github.com/getsops/sops) + [age](https://github.com/FiloSottile/age):
`env/enc/<dev|prod>.env.enc` is committed; `just env-use <name>` decrypts it to
`env/dec/<name>.env` (gitignored, mode 0600) and symlinks `./.env` to it. The
Nix dev shell provides the tooling, `just env-audit` runs keyless in CI, and
containers decrypt at `docker run` — never at build. See [`env/README.md`](env/README.md).
