# CCOS Core repository agent instructions

Before repository changes, fetch and read the persistent off-main product/ecosystem roadmap:

```bash
git fetch origin agent/ecosystem-roadmap && \
git show origin/agent/ecosystem-roadmap:.agent/CCOS_CORE_ECOSYSTEM_ROADMAP.yaml
```

Treat root `AGENTS.md` as mandatory bootstrap policy. If the roadmap is unavailable, fail closed for major Core-boundary, replay, storage-format, cross-product sync, cross-repository integration, or merge decisions.

Keep Enterprise governance and Research Lab self-modification/fusion outside Core's default semantics. Preserve deterministic replay and exact repository/worktree/session identity for coding-agent context integration.
