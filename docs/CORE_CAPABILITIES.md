# CCOS Core capability matrix

This matrix is the authoritative map from the root `Cargo.toml` feature
manifest to shipped Core code. It deliberately distinguishes a compiled
feature from a product claim: an absent feature is not available through a
license, command or undocumented namespace.

| Cargo feature | Default | Shipped Core capability | Source / contract evidence |
| --- | --- | --- | --- |
| `syn-parser` | yes | Rust AST ingestion; heuristic fallback without defaults | `src/parser.rs`; parser unit tests |
| `llm` | no | async runtime, workspace scanner, CLI and MCP host | `src/llm.rs`, `src/workspace.rs`; `tests/cli.rs`, `tests/workspace_scanner.rs` |
| `mimalloc` | no | opt-in allocator for measurement | `src/main.rs`; build matrix |
| `learned-embed` | no | deterministic LSA projection | `src/lsa.rs`, `src/external_memory.rs`; embedding tests |
| `license` | no | offline Ed25519 token verification | `src/license.rs`; Ed25519 verifier tests |
| `license-pq` | no | offline SLH-DSA token verification | `src/license.rs`; release-profile SLH-DSA tests |
| `signed-sync` | no | signed, TOFU-pinned workspace bundles | `src/agent_session.rs`; signed-sync tests |
| `slhav2` | no | distilled zero-dependency tile memory provider | `crates/ccos-memory-runtime`; provider and tier-gating tests |
| `neural-embed` | no | quarantined local embeddings endpoint | `src/neural_embed.rs`, `src/egress.rs`; egress-policy tests |

The workspace contains only `ccos-core` and
`crates/ccos-memory-runtime`. The latter is a distilled Core-side provider;
it does not contain or depend on SciRust.

## Explicitly external

The former names `slhav2-full`, `octasoma`, `octacore`, `rsi`,
`rsi-dgm`, `rsi-full`, `pro-default` and `all-full` are absent from the
feature manifest. Full SLHAv2, OctaSoma/OctaCore, CERVO/RSI, Forge,
generated-code execution and self-modification belong to their owning
repositories or CCOS Research Lab. Core neither vendors nor exposes them.

## Verification

`tests/capability_matrix_contract.rs` checks that every feature row still
exists in `Cargo.toml`, that removed fused names do not reappear as build
features, and that README keeps the Core/Research boundary. CI also runs
`scripts/check-no-research-components.sh` against metadata, dependency trees
and source symbols.
