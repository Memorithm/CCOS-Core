# CCOS v0.5 — Memory sovereignty and ports

Status: **accepted architectural direction; implementation in progress**.

## Decision

CCOS Core is sovereign over causal truth, event sourcing, deterministic replay,
hard policy and causal graph semantics.

Optional semantic or episodic memory engines are external capabilities. They
integrate through backend-neutral ports defined by `ccos_core::memory::ports` and
implemented by adapter crates outside `ccos-core`.

The target dependency rule is:

```text
external memory engine
        ^
        |
consumer-side adapter
        ^
        |
CCOS Core ports
```

There is no reverse edge from CCOS Core to an external memory implementation.

## Hard invariants

1. `ccos-core` must not directly or transitively depend on an external semantic
   memory implementation or on SciRust in any Cargo feature combination.
2. Backend-specific crate names, index types, store formats, feature flags and
   licensing policy do not belong in the public CCOS Core domain API.
3. External retrieval results are **observations**. A score is not authority,
   probability, causal weight or permission to bypass deterministic CCOS rules.
4. Causal truth is never delegated through `SemanticMemoryProvider` or
   `EpisodicProvider`.
5. Product concerns such as tenant isolation, RBAC, key management, retention
   policy and right-to-forget orchestration live in Enterprise/Research layers.
6. An adapter must translate native backend records into CCOS-owned port types;
   native backend index or graph types must not leak across the boundary.
7. A final process must contain a single Cargo identity for `ccos-core`. A
   consumer workspace must not combine a path instance and a second Git/registry
   instance of the same Core when an adapter implements Core traits.

## Port responsibilities

`SemanticMemoryProvider` owns only the backend operations needed to store and
retrieve semantic candidates.

`EpisodicProvider` owns only append/recall operations for episode-like external
memory.

`MemoryLifecycleProvider` exposes primitive tombstone/purge operations. The
policy deciding *when* those operations happen remains outside Core.

`MemoryScope` is intentionally opaque. Enterprise may map it to a nested
Tenant/Workspace/Agent scope without teaching Core about its tenancy model.

## Adapter placement

The production adapter belongs on the consumer side (for example an Enterprise
or Research workspace crate) so that it depends on the exact `ccos-core` package
identity already present in that workspace plus the canonical external memory
crate revision.

This avoids both abstraction leakage and duplicate trait identities.

## Migration from v0.4

The v0.4 tree still contains a direct optional semantic-memory integration. The
v0.5 migration removes, atomically with its lockfile update:

- the direct optional dependency from `ccos-core`;
- its backend-specific Cargo feature and example target;
- the backend-specific Core module;
- backend-specific license symbols/policy from Core;
- any remaining source or dependency-graph exceptions.

CI must then enforce the absence of those dependencies for default,
`--no-default-features`, and `--all-features` builds.

## Non-goals

This decision does not remove CCOS's own deterministic lexical/semantic recall
primitives. It only prevents an external semantic/episodic engine from becoming
part of the Core dependency or authority boundary.