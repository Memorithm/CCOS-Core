//! Telemetry contract — the backend-neutral vocabulary shared by the runtime,
//! its backends, and the monitors. Contains **no** SLHAv2 (`scirust`) types.

/// Which backend produced a result. The runtime stays backend-agnostic; this is
/// purely for telemetry / routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryBackend {
    SlhaV2,
    Mock,
    External,
}

/// Lifecycle state of a page, owned by the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryState {
    /// Full representation available (latent + residual).
    Hot,
    /// Reduced logical representation (residual ignored; fixed resident slot retained).
    Warm,
    /// Paged out of active memory (persistence handled externally by CCOS).
    Cold,
}

/// Fidelity available after a compressed page is restored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreFidelity {
    /// The discarded residual cannot be reconstructed from the resident page.
    Approximate,
    /// Every discarded component can be restored from separately qualified storage.
    Exact,
}

/// The outcome of a compression (state transition) as exposed to the runtime.
///
/// Logical bytes describe meaningful codec content. Resident bytes describe the
/// in-memory page slot owned by the backend. A logical reduction is not evidence
/// that an allocator returned memory or that process RSS decreased.
#[derive(Debug, Clone)]
pub struct CompressionResult {
    pub backend: MemoryBackend,
    pub previous_state: MemoryState,
    pub new_state: MemoryState,
    pub logical_bytes_before: usize,
    pub logical_bytes_after: usize,
    pub resident_bytes_before: usize,
    pub resident_bytes_after: usize,
    /// True only when the backend actually releases resident storage.
    pub resident_storage_released: bool,
    /// Fidelity of the currently implemented restore path.
    pub restore_fidelity: RestoreFidelity,
    /// Measured semantic degradation (backend-defined proxy).
    pub quality_delta: f32,
    pub cpu_cycles: u64,
    /// Representation exposed to the runtime. The backend generates it, the
    /// Monitor (via the runtime) consumes it.
    pub representation: Option<[f32; 128]>,
}
