// ═════════════════════════════════════════════════════════════════════
// Built-in Compression Stages
// ═════════════════════════════════════════════════════════════════════
//
// Three stages shipped by default:
//
//  1. ContentClassifier — detects instruction-like content; if NOT
//     instruction content, marks unchanged so downstream stages skip.
//  2. DedupStage        — session-aware cross-turn deduplication.
//  3. LogCrunchStage    — static minification (comments, decoration,
//                         whitespace collapse).

mod content_classifier;
mod dedup;
mod log_crunch;

pub use content_classifier::ContentClassifier;
pub use dedup::DedupStage;
pub use log_crunch::LogCrunchStage;
