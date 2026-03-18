// =============================================================================
// Pipeline Module — Public API surface for the Algorithmic AI Gateway.
//
// This module exposes the pipeline's core components:
//
// - `context`      — Data models (PipelineContext, IntentClassification, etc.)
// - `traits`       — Algorithmic interface definitions (Embedder, Reranker, …)
// - `stubs`        — Placeholder implementations for development / testing
// - `concurrency`  — Adaptive concurrency limiter (Layer 0 — Ops)
// - `orchestrator` — The main pipeline execution engine
// =============================================================================

pub mod concurrency;
pub mod context;
pub mod implementations;
pub mod orchestrator;
pub mod traits;

#[cfg(test)]
pub mod stubs;

// Re-export key types for ergonomic imports.
#[allow(unused_imports)]
pub use concurrency::{AdaptiveConcurrencyLimiter, ConcurrencyConfig};
#[allow(unused_imports)]
pub use context::{IntentClassification, PipelineContext, PipelineResponse, ProcessingLogEntry};
pub use orchestrator::{PipelineConfig, execute_pipeline};
pub use traits::AlgorithmSuite;
