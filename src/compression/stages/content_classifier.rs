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
        use crate::compression::cache::InstructionCache;
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
        use crate::compression::cache::InstructionCache;
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

    // ── Marker coverage ─────────────────────────────────────────

    #[test]
    fn detects_copilot_instructions_marker() {
        let text = "# Copilot Instructions\ncopilot-instructions for this repo.\n".to_string()
            + &"content ".repeat(40);
        assert_eq!(ContentClassifier::classify(&text), ContentKind::Instruction);
    }

    #[test]
    fn detects_claude_md_marker() {
        let text = "This is CLAUDE.MD content for the project.\n".to_string()
            + &"instructions ".repeat(30);
        assert_eq!(ContentClassifier::classify(&text), ContentKind::Instruction);
    }

    #[test]
    fn detects_environment_context_marker() {
        let text = "<environment_context>\nOS: Linux\nCWD: /home/user\n</environment_context>\n"
            .to_string()
            + &"x ".repeat(100);
        assert_eq!(ContentClassifier::classify(&text), ContentKind::Instruction);
    }

    #[test]
    fn detects_session_context_marker() {
        let text = "<session_context>\nSession folder: /tmp/session\n</session_context>\n"
            .to_string()
            + &"x ".repeat(100);
        assert_eq!(ContentClassifier::classify(&text), ContentKind::Instruction);
    }

    #[test]
    fn detects_tool_calling_marker() {
        let text = "<tool_calling>\nYou can call tools.\n</tool_calling>\n".to_string()
            + &"x ".repeat(100);
        assert_eq!(ContentClassifier::classify(&text), ContentKind::Instruction);
    }

    #[test]
    fn detects_available_skills_marker() {
        let text = "<available_skills>\n<skill>pdf</skill>\n</available_skills>\n".to_string()
            + &"x ".repeat(100);
        assert_eq!(ContentClassifier::classify(&text), ContentKind::Instruction);
    }

    #[test]
    fn detects_code_change_instructions_marker() {
        let text = "<code_change_instructions>\n<rules_for_code_changes>Be careful</rules_for_code_changes>\n</code_change_instructions>\n"
            .to_string() + &"x ".repeat(100);
        assert_eq!(ContentClassifier::classify(&text), ContentKind::Instruction);
    }

    #[test]
    fn detects_you_are_an_ai_marker() {
        let text = "You are an AI programming assistant built by GitHub.\n".to_string()
            + &"Be helpful and precise. ".repeat(20);
        assert_eq!(ContentClassifier::classify(&text), ContentKind::Instruction);
    }

    // ── Edge cases ──────────────────────────────────────────────

    #[test]
    fn exactly_200_bytes_without_markers_is_conversational() {
        let text = "a".repeat(200);
        assert_eq!(
            ContentClassifier::classify(&text),
            ContentKind::Conversational
        );
    }

    #[test]
    fn exactly_199_bytes_is_conversational() {
        let text = "a".repeat(199);
        assert_eq!(
            ContentClassifier::classify(&text),
            ContentKind::Conversational
        );
    }

    #[test]
    fn empty_string_is_conversational() {
        assert_eq!(ContentClassifier::classify(""), ContentKind::Conversational);
    }

    #[test]
    fn case_insensitive_marker_detection() {
        let text = "YOU ARE AN AI assistant for this project.\n".to_string() + &"x ".repeat(100);
        assert_eq!(ContentClassifier::classify(&text), ContentKind::Instruction);
    }

    #[test]
    fn marker_in_middle_of_text_detected() {
        let text = "Some preamble text. ".repeat(5)
            + "<custom_instructions>payload</custom_instructions>"
            + &" More text.".repeat(10);
        assert_eq!(ContentClassifier::classify(&text), ContentKind::Instruction);
    }
}
