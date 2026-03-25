// ═════════════════════════════════════════════════════════════════════
// Stage 1 — ContentClassifier
// ═════════════════════════════════════════════════════════════════════
//
// Gate stage: detects whether the text is an instruction payload.
// If NOT instruction content, returns unchanged so downstream stages
// (dedup, log_crunch) skip work on conversational messages.

use crate::compression::pipeline::{CompressionInput, CompressionStage, StageOutput};

/// Minimum byte length to consider as instruction content.
const MIN_INSTRUCTION_LEN: usize = 200;

/// Markers that indicate agentic instruction payloads.
const INSTRUCTION_MARKERS: &[&str] = &[
    "custom_instruction",
    "copilot_instruction",
    "claude.md",
    "copilot-instructions",
    "<custom_instructions>",
    "</custom_instructions>",
    "<available_skills>",
    "# copilot instructions",
    "you are an ai",
    "you are a helpful",
    "code_change_instructions",
    "rules_for_code_changes",
    "<environment_context>",
    "version_information",
    "<session_context>",
    "<tool_calling>",
    "<thinking",
    "model_information",
    "<tips_and_tricks>",
];

/// Content classification result stored in the stage output metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    /// Text is an instruction payload — eligible for compression.
    Instruction,
    /// Text is conversational — skip compression.
    Conversational,
}

/// Gate stage that classifies content as instruction vs conversational.
///
/// If the text is not instruction content, this stage returns
/// `StageOutput::unchanged()` which signals downstream stages to
/// skip processing.
pub struct ContentClassifier;

impl ContentClassifier {
    /// Classify text without running as a pipeline stage.
    pub fn classify(text: &str) -> ContentKind {
        if text.len() < MIN_INSTRUCTION_LEN {
            return ContentKind::Conversational;
        }

        let lower = text.to_lowercase();
        let hits = INSTRUCTION_MARKERS
            .iter()
            .filter(|m| lower.contains(*m))
            .count();

        if hits >= 1 {
            ContentKind::Instruction
        } else {
            ContentKind::Conversational
        }
    }
}

impl CompressionStage for ContentClassifier {
    fn name(&self) -> &'static str {
        "classifier"
    }

    fn process(&self, _input: &CompressionInput, text: &str) -> StageOutput {
        match Self::classify(text) {
            ContentKind::Instruction => {
                // Pass through unchanged — downstream stages will handle compression
                StageOutput::unchanged(text)
            }
            ContentKind::Conversational => {
                // Short-circuit: no compression needed for conversational content
                StageOutput {
                    text: text.to_string(),
                    modified: false,
                    bytes_saved: 0,
                    short_circuit: true,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_conversational() {
        assert_eq!(
            ContentClassifier::classify("Hello world"),
            ContentKind::Conversational
        );
    }

    #[test]
    fn instruction_marker_detected() {
        let text =
            "You are an AI assistant. <custom_instructions>Be helpful.</custom_instructions>\n"
                .to_string()
                + &"x".repeat(300);
        assert_eq!(ContentClassifier::classify(&text), ContentKind::Instruction);
    }

    #[test]
    fn long_text_without_markers_is_conversational() {
        let text = "The quick brown fox jumped over the lazy dog. ".repeat(20);
        assert_eq!(
            ContentClassifier::classify(&text),
            ContentKind::Conversational
        );
    }

    #[test]
    fn classifier_stage_short_circuits_on_conversational() {
        use crate::core::context_compress::InstructionCache;
        let cache = InstructionCache::new();
        let input = CompressionInput {
            session_scope: None,
            instruction_cache: &cache,
        };
        let stage = ContentClassifier;
        let out = stage.process(&input, "Hello world");
        assert!(!out.modified);
        assert!(out.short_circuit);
    }

    #[test]
    fn classifier_stage_passes_instruction_content() {
        use crate::core::context_compress::InstructionCache;
        let cache = InstructionCache::new();
        let input = CompressionInput {
            session_scope: None,
            instruction_cache: &cache,
        };
        let stage = ContentClassifier;
        let text = "You are an AI assistant. <custom_instructions>Long</custom_instructions>\n"
            .to_string()
            + &"x".repeat(300);
        let out = stage.process(&input, &text);
        assert!(!out.modified);
        assert!(!out.short_circuit);
    }
}
