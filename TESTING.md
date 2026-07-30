# MCP contract test matrix

The automated suite exercises protocol behavior through both pure dispatch tests
and the real Axum router/middleware stack.

| Area | Covered cases |
| --- | --- |
| Authentication | missing secret configuration fails closed; absent/wrong request secret rejected; constant-time exact match |
| Browser Origin | native client with no `Origin`; exact normalized allowlist; untrusted Origin rejected; wildcard configuration rejected |
| Body limits | request larger than 64 KiB returns HTTP 413 |
| JSON-RPC envelope | exact `2.0`; string and integer IDs; invalid version echoes valid ID; null/fraction/bool/object/array IDs rejected with `-32600` |
| Notifications | initialized, cancelled, and unknown future notifications return HTTP 202 with no body |
| MCP lifecycle | initialization negotiates supported protocol revisions; unsupported `MCP-Protocol-Version` rejected |
| Discovery | deterministic one-tool catalog; object schema; `additionalProperties: false`; read-only/non-destructive/idempotent/closed-world annotations |
| Tool calls | structured `ping` response; missing/unknown tool; non-object arguments; oversized/control-character messages |
| Method errors | unknown method returns `-32601`; invalid tool parameters return `-32602` |
| Probes | `/health` and `/ready` remain public while `/mcp` stays protected |

Run locally:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo build --locked --release
```

The CI workflow additionally runs `actionlint` and `cargo audit --deny warnings`.
Tests are hermetic and do not require the Kubernetes cluster, AntiCapTrad
credentials, or public network access.
