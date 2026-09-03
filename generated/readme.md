# Generated output — do not edit directly

Everything in this directory except this notice is derivative output. Existing file headers and `.cli-flags.toml`, where applicable, identify the current operational generator inputs. Edit repository-owned source inputs and regenerate instead of patching Dart, Gleam, Rust, TypeScript, schema, or documentation output by hand.

## Contract authority

TypeSpec and JSON Schema/OpenAPI are independent, human-authored peer authorities for semantic cross-language contracts. Neither may be generated from the other as the ultimate source of truth. A generated contract change is mergeable only when both authorities change together, their normalized outputs agree, and a machine-readable reconciliation receipt is committed under `../contracts/parity/`.

A schema placed below `generated/` is still derivative output and never counts as the human-authored JSON Schema/OpenAPI authority. The generated-policy workflow enforces these requirements for future generated changes. Existing output predates the complete peer-authority migration and must not be represented as already compliant.

## Service boundary

Generated bindings or documentation do not replace the Rust service implementation and do not imply that every process exposes a public API. Public, internal/server-only, edge-only, and client-visible contracts must remain explicitly separated in the authority sources.

## Regenerate and freeze

Before regeneration, temporarily make the tree writable:

```sh
chmod -R u+w generated
```

Run the documented generator, review the full diff, then make the tree read-only again:

```sh
find generated -depth -exec chmod a-w {} +
```

Git persists only the regular-versus-executable distinction, not arbitrary owner-write bits. A fresh checkout can therefore be writable even when a working tree was frozen locally. CI is the durable merge control; `chmod a-w` is an additional local deterrent.