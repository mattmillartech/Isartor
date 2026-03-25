// ═════════════════════════════════════════════════════════════════════
// CompressionPipeline — Stage executor and data types.
// ═════════════════════════════════════════════════════════════════════

use std::time::Instant;

// ── Stage trait ─────────────────────────────────────────────────────

/// A single compression stage in the pipeline.
///
/// Stages are stateless: all mutable state (dedup caches, etc.) is
/// passed via the `CompressionInput` or held in shared infrastructure
/// (e.g. `InstructionCache` on `AppState`).
///
/// A stage that doesn't apply returns `StageOutput::unchanged()`.
pub trait CompressionStage: Send + Sync {
    /// Human-readable name for telemetry (e.g. "content_classifier").
    fn name(&self) -> &'static str;

    /// Process the text and return a `StageOutput`.
    ///
    /// `input` carries the current text, session scope, and any
    /// shared resources the stage needs.
    fn process(&self, input: &CompressionInput, text: &str) -> StageOutput;
}

// ── Input / Output types ────────────────────────────────────────────

/// Immutable context supplied to every stage.
pub struct CompressionInput<'a> {
    /// Session scope key for dedup (if available).
    pub session_scope: Option<&'a str>,

    /// Shared dedup cache from `AppState`.
    pub instruction_cache: &'a crate::compression::cache::InstructionCache,
}

/// Result of a single stage's `process()` call.
#[derive(Debug, Clone)]
pub struct StageOutput {
    /// The (possibly modified) text after this stage.
    pub text: String,

    /// Whether this stage modified the text.
    pub modified: bool,

    /// Bytes saved by this stage (0 if unmodified).
    pub bytes_saved: usize,

    /// True if this stage replaced the entire text (e.g. dedup).
    /// When set, subsequent stages should skip further processing.
    pub short_circuit: bool,
}

impl StageOutput {
    /// Convenience constructor for no-op stages.
    pub fn unchanged(text: &str) -> Self {
        Self {
            text: text.to_string(),
            modified: false,
            bytes_saved: 0,
            short_circuit: false,
        }
    }
}

/// Telemetry report for a single stage execution.
#[derive(Debug, Clone)]
pub struct StageReport {
    /// Stage name.
    pub name: &'static str,
    /// Whether the stage modified content.
    pub modified: bool,
    /// Bytes saved by this stage.
    pub bytes_saved: usize,
    /// Execution time in microseconds.
    pub duration_us: u64,
    /// Whether the stage short-circuited the pipeline.
    pub short_circuited: bool,
}

/// Final output of the full pipeline.
#[derive(Debug)]
pub struct CompressionOutput {
    /// The final compressed text.
    pub text: String,
    /// Whether any stage modified the text.
    pub modified: bool,
    /// Total bytes saved across all stages.
    pub total_bytes_saved: usize,
    /// Summary strategy label for telemetry.
    pub strategy: String,
    /// Per-stage telemetry reports.
    pub stages: Vec<StageReport>,
    /// Total pipeline execution time in microseconds.
    pub total_duration_us: u64,
}

// ── Pipeline executor ───────────────────────────────────────────────

/// An ordered sequence of compression stages.
///
/// Stages run in insertion order. If a stage sets `short_circuit = true`,
/// subsequent stages are skipped.
pub struct CompressionPipeline {
    stages: Vec<Box<dyn CompressionStage>>,
}

impl CompressionPipeline {
    /// Create an empty pipeline.
    pub fn new() -> Self {
        Self { stages: Vec::new() }
    }

    /// Append a stage to the end of the pipeline.
    pub fn add_stage(mut self, stage: Box<dyn CompressionStage>) -> Self {
        self.stages.push(stage);
        self
    }

    /// Execute all stages in order on the given text.
    pub fn execute(&self, input: &CompressionInput, original_text: &str) -> CompressionOutput {
        let pipeline_start = Instant::now();
        let mut current_text = original_text.to_string();
        let mut reports: Vec<StageReport> = Vec::with_capacity(self.stages.len());
        let mut total_saved: usize = 0;
        let mut any_modified = false;
        let mut strategy_parts: Vec<&str> = Vec::new();

        for stage in &self.stages {
            let stage_start = Instant::now();
            let output = stage.process(input, &current_text);
            let duration_us = stage_start.elapsed().as_micros() as u64;

            reports.push(StageReport {
                name: stage.name(),
                modified: output.modified,
                bytes_saved: output.bytes_saved,
                duration_us,
                short_circuited: output.short_circuit,
            });

            if output.modified {
                total_saved += output.bytes_saved;
                any_modified = true;
                strategy_parts.push(stage.name());
                current_text = output.text;
            }

            if output.short_circuit {
                break;
            }
        }

        let strategy = if strategy_parts.is_empty() {
            "none".to_string()
        } else {
            strategy_parts.join("+")
        };

        CompressionOutput {
            text: current_text,
            modified: any_modified,
            total_bytes_saved: total_saved,
            strategy,
            stages: reports,
            total_duration_us: pipeline_start.elapsed().as_micros() as u64,
        }
    }

    /// Returns the number of stages in the pipeline.
    pub fn len(&self) -> usize {
        self.stages.len()
    }

    /// Returns true if the pipeline has no stages.
    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }
}

impl Default for CompressionPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compression::cache::InstructionCache;

    /// A test stage that uppercases text.
    struct UpperStage;
    impl CompressionStage for UpperStage {
        fn name(&self) -> &'static str {
            "upper"
        }
        fn process(&self, _input: &CompressionInput, text: &str) -> StageOutput {
            let upper = text.to_uppercase();
            let saved = 0; // no size change
            StageOutput {
                text: upper,
                modified: true,
                bytes_saved: saved,
                short_circuit: false,
            }
        }
    }

    /// A test stage that short-circuits.
    struct ShortCircuitStage;
    impl CompressionStage for ShortCircuitStage {
        fn name(&self) -> &'static str {
            "short_circuit"
        }
        fn process(&self, _input: &CompressionInput, _text: &str) -> StageOutput {
            StageOutput {
                text: "[replaced]".to_string(),
                modified: true,
                bytes_saved: 100,
                short_circuit: true,
            }
        }
    }

    /// A no-op stage.
    struct NoopStage;
    impl CompressionStage for NoopStage {
        fn name(&self) -> &'static str {
            "noop"
        }
        fn process(&self, _input: &CompressionInput, text: &str) -> StageOutput {
            StageOutput::unchanged(text)
        }
    }

    #[test]
    fn empty_pipeline_is_noop() {
        let cache = InstructionCache::new();
        let input = CompressionInput {
            session_scope: None,
            instruction_cache: &cache,
        };
        let pipeline = CompressionPipeline::new();
        let out = pipeline.execute(&input, "hello");
        assert!(!out.modified);
        assert_eq!(out.text, "hello");
        assert_eq!(out.strategy, "none");
    }

    #[test]
    fn stages_execute_in_order() {
        let cache = InstructionCache::new();
        let input = CompressionInput {
            session_scope: None,
            instruction_cache: &cache,
        };
        let pipeline = CompressionPipeline::new().add_stage(Box::new(UpperStage));
        let out = pipeline.execute(&input, "hello");
        assert!(out.modified);
        assert_eq!(out.text, "HELLO");
        assert_eq!(out.strategy, "upper");
    }

    #[test]
    fn short_circuit_skips_later_stages() {
        let cache = InstructionCache::new();
        let input = CompressionInput {
            session_scope: None,
            instruction_cache: &cache,
        };
        let pipeline = CompressionPipeline::new()
            .add_stage(Box::new(ShortCircuitStage))
            .add_stage(Box::new(UpperStage)); // should not run
        let out = pipeline.execute(&input, "hello");
        assert!(out.modified);
        assert_eq!(out.text, "[replaced]");
        assert_eq!(out.stages.len(), 1); // only short_circuit ran (reported)
        assert!(out.stages[0].short_circuited);
    }

    #[test]
    fn noop_stage_preserves_text() {
        let cache = InstructionCache::new();
        let input = CompressionInput {
            session_scope: None,
            instruction_cache: &cache,
        };
        let pipeline = CompressionPipeline::new()
            .add_stage(Box::new(NoopStage))
            .add_stage(Box::new(UpperStage));
        let out = pipeline.execute(&input, "hello");
        assert!(out.modified);
        assert_eq!(out.text, "HELLO");
        assert_eq!(out.stages.len(), 2);
        assert!(!out.stages[0].modified);
        assert!(out.stages[1].modified);
        assert_eq!(out.strategy, "upper");
    }

    #[test]
    fn strategy_joins_multiple_stages() {
        let cache = InstructionCache::new();
        let input = CompressionInput {
            session_scope: None,
            instruction_cache: &cache,
        };
        // Both modify, neither short-circuits
        let pipeline = CompressionPipeline::new()
            .add_stage(Box::new(UpperStage))
            .add_stage(Box::new(UpperStage)); // idempotent but "modified" is always true
        let out = pipeline.execute(&input, "hello");
        assert_eq!(out.strategy, "upper+upper");
    }
}
