# CCOS Core Agent Bootstrap Contract

Before autonomous coding, Core feature/dependency changes, storage/replay changes, Enterprise/Research synchronization, SoulSystem/Orchestrator integration, PR creation, or merge decisions, read:

```bash
git fetch origin agent/ecosystem-roadmap && \
git show origin/agent/ecosystem-roadmap:.agent/CCOS_CORE_ECOSYSTEM_ROADMAP.yaml
```

If the roadmap cannot be fetched or read, fail closed for major Core-boundary, replay, storage-format, cross-product sync, cross-repository integration, or merge decisions. Read-only diagnosis is allowed.

## Product boundary

CCOS Core is the stable deterministic causal-context/replay kernel. Enterprise governance/multi-tenancy belongs to CCOS Enterprise. RSI/Forge/generated-code/fused experiments belong to CCOS Research Lab. Those responsibilities must not silently leak into Core's default dependency graph.

The current repository documentation contains both the three-product boundary and legacy/fused `CCOS_EXTENDED` material. Verify code/features/dependencies before treating fused documentation as current Core ownership.

CCOS may provide context memory to SoulSystem or Memorithm Orchestrator, but those products retain agent/tool/PR workflow ownership. Bind context logs to exact repository/worktree/session identity.

Preserve deterministic replay, observable lossy compaction, hash-verified cold storage, and honest retrieval evidence. Required CI must be green on the exact PR head before merge.

Reread the roadmap at every session start, before Core/storage/replay changes, before product-line sync, before ecosystem integrations, and before relevant PR/merge decisions.

Do not merge the roadmap itself into `main` unless the user explicitly requests it.
