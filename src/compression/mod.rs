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
// Built-in stages:
//   - `ContentClassifier`  — detects instruction vs. conversational content
//   - `DedupStage`         — session-aware cross-turn deduplication
//   - `LogCrunchStage`     — static minification (comments, decoration, whitespace)
// ═════════════════════════════════════════════════════════════════════

pub mod pipeline;
pub mod stages;

pub use pipeline::{
    CompressionInput, CompressionOutput, CompressionPipeline, StageOutput, StageReport,
};
pub use stages::{ContentClassifier, DedupStage, LogCrunchStage};
