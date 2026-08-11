//! # CCOS — Causal Context Operating System
//!
//! CCOS is an experimental kernel that treats an agent's working context as a
//! causal, event-sourced memory system. The causal graph, replay contract and hard
//! state remain owned by CCOS. Optional external capabilities integrate through
//! backend-neutral ports rather than backend-specific dependencies in the domain
//! API.
//!
//! ## Quick start
//!
//! ```
//! use ccos_core::{CcosMemory, ExternalMemory, Recall};
//!
//! let mut mem = CcosMemory::new();
//! mem.ingest_source("src/db.rs", "pub fn query() -> i64 { 0 }\n");
//! let window = mem.recall(&Recall::working_set(), 1024);
//! assert!(!window.items.is_empty());
//! ```
//!
//! [`memory::ports`] contains the stable hexagonal boundary for optional
//! semantic/episodic providers. Implementations live outside `ccos-core`; a
//! provider returns observations and never acquires causal authority merely by
//! implementing a port.

pub mod adversarial;
pub mod agent_session;
pub mod cold_index;
pub mod compressor;
pub mod conformal;
pub mod consensus;
pub mod distributed_event_log;
pub mod drift;
pub mod dtw;
pub mod egress;
pub mod embeddings;
pub mod event_log;
pub mod eviction_policy;
pub mod external_memory;
pub mod extractor;
pub mod guard;
pub mod hashing_tokenizer;
pub mod incremental;
pub mod injection_classifier;
pub mod license;
pub mod lingam;
#[cfg(feature = "llm")]
pub mod llm;
pub mod lsa;
pub mod lzss;
pub mod mcp;

// Keep the large causal-memory implementation physically stable while exposing
// the v0.5 hexagonal ports at `ccos_core::memory::ports`. The private module is an
// implementation detail; public memory types are re-exported through the
// canonical `memory` namespace exactly as before.
#[path = "memory.rs"]
mod memory_impl;

/// CCOS causal memory and backend-neutral extension ports.
pub mod memory {
    pub use crate::memory_impl::*;

    /// Ports implemented by optional external semantic/episodic adapters.
    pub mod ports;
}

pub mod migrate;

// Quarantined neural embedder (off by default). It remains a CCOS-owned
// capability and does not change the memory-provider dependency boundary.
#[cfg(feature = "neural-embed")]
pub mod neural_embed;

pub mod parser;
pub mod persist;
pub mod postmortem;
pub mod query;
pub mod retrieval;
pub mod retrodict;
pub mod sanitizer;
pub mod setup;
pub mod spectral;
pub mod trace;
pub mod util;

// CCOS autonomous context runtime.
pub mod agents;
pub mod benchmark;
pub mod causal_flash;
pub mod persistence;
pub mod scheduler;
#[cfg(feature = "llm")]
pub mod workspace;

// Context Region Engine.
pub mod context_policy;
pub mod context_region;
#[cfg(feature = "llm")]
pub mod eval;
pub mod experiment;
pub mod region_engine;
pub mod region_metrics;

// Core re-exports.
pub use agent_session::AgentSession;
pub use event_log::EventLog;
pub use external_memory::{
    CcosMemory, ExternalMemory, IngestReport, Integrity, MemoryError, Recall, RecallItem,
    RecallWindow,
};
pub use memory::{EdgeType, GraphEdge, GraphNode, MemoryGraph, NodeId, NodeType, ScoringWeights};
pub use persist::KernelSnapshot;
