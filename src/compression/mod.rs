// ═════════════════════════════════════════════════════════════════════
// Compression Pipeline — Modular, staged content compression for L2.5.
//
// Architecture follows the Fusion Pipeline pattern:
//   - Each stage is a stateless `CompressionStage` trait object.
//   - Stages receive an immutable `CompressionInput` and return a
//     `StageOutput` describing what they changed.
//   - The pipeline executor runs stages in order, threading the
//     accumulated text through each stage and collecting telemetry.
//
// Modules:
//   - `cache`     — InstructionCache (per-session dedup state)
//   - `pipeline`  — CompressionPipeline executor and stage trait
//   - `stages`    — Built-in stages (ContentClassifier, Dedup, LogCrunch)
//   - `optimize`  — Request-body rewriting (JSON extraction + pipeline)
// ═════════════════════════════════════════════════════════════════════

pub mod cache;
pub mod optimize;
pub mod pipeline;
pub mod stages;

// Re-export key types for ergonomic imports.
pub use cache::{InstructionCache, hash_instructions};
pub use optimize::{OptimizeResult, build_pipeline, optimize_request_body};
pub use pipeline::{
    CompressionInput, CompressionOutput, CompressionPipeline, CompressionStage, StageOutput,
    StageReport,
};
pub use stages::{ContentClassifier, DedupStage, LogCrunchStage};
