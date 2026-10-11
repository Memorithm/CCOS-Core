# ccos-memory-runtime

Neutral cognitive-memory middleware for CCOS — **zero external dependencies**.

```text
CCOS Core ─▶ MemoryPolicy ─▶ ccos-memory-runtime ─▶ MemoryProvider ─▶ SlhaAdapter
```

- **`traits::MemoryProvider`** — the only interface the runtime speaks.
- **`telemetry`** — `MemoryState` (HOT/WARM/COLD), `MemoryBackend`,
  `CompressionResult` (backend-neutral; the backend fills `representation`, a
  monitor consumes it).
- **`AlignedMemoryPage`** — 128-byte, 128-aligned opaque page.
- **`backend::slha_adapter::SlhaAdapter`** — a self-contained SLHAv2 tile backend.

## Why zero dependencies

`scirust` (the SLHAv2 kernel) is a development toolbox, not a runtime dependency.
The one piece needed here — the 128-byte tile ABI and its grouped-INT4 decode —
is **vendored** (copied) into `slha_adapter.rs`. So neither CCOS nor this crate
ever pulls `scirust`; `cargo tree` prints only this crate.

## Lifecycle

`compress` moves a page **HOT → WARM**: it drops the 32-byte residual from
the logical payload (**128 → 96 B**) while preserving the latent, but keeps the
fixed resident `AlignedMemoryPage` slot unchanged. It therefore does not claim
allocator or process-RSS savings. The returned `representation` is the
dequantized latent. `restore` brings the page back **WARM → HOT** approximately
(latent-only — the residual is not recoverable). `evict_page` marks it **COLD**
(CCOS persists it externally).

## Quick check

```bash
cargo test        # 5/5
cargo clippy --all-targets -- -D warnings
cargo tree        # only `ccos-memory-runtime` — no deps
```

## Integration into CCOS

See [`INTEGRATION.md`](INTEGRATION.md) — it drives an automated integrator to add
this crate as a workspace member behind an opt-in `slhav2` feature (default CCOS
build stays scirust-free).

Licensed under `PolyForm-Noncommercial-1.0.0`.
