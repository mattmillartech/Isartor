// ═════════════════════════════════════════════════════════════════════
// Stage 2 — DedupStage
// ═════════════════════════════════════════════════════════════════════
//
// Session-aware cross-turn deduplication.  Hashes the instruction
// text and checks the shared `InstructionCache`.  On a repeat, the
// full text is replaced with a compact hash reference, saving
// thousands of tokens.  On first-seen, records the hash for future
// turns.  Short-circuits on dedup hit since the text is replaced.

use crate::compression::cache::hash_instructions;
use crate::compression::pipeline::{CompressionInput, CompressionStage, StageOutput};

/// Cross-turn instruction deduplication stage.
pub struct DedupStage;

impl CompressionStage for DedupStage {
    fn name(&self) -> &'static str {
        "dedup"
    }

    fn process(&self, input: &CompressionInput, text: &str) -> StageOutput {
        let Some(scope) = input.session_scope else {
            // No session scope — can't dedup, pass through
            return StageOutput::unchanged(text);
        };

        let hash = hash_instructions(text);

        if let Some(turn) = input.instruction_cache.check_and_update(scope, &hash) {
            // Seen before — replace with compact reference
            let reference = format!(
                "[Context instructions unchanged since turn 1 (hash={hash}, turn={turn}). \
                 Follow the same instructions as before.]"
            );
            let saved = text.len().saturating_sub(reference.len());
            StageOutput {
                text: reference,
                modified: true,
                bytes_saved: saved,
                short_circuit: true, // no point minifying a one-liner
            }
        } else {
            // First time seeing this hash for this session — pass through
            StageOutput::unchanged(text)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compression::cache::InstructionCache;

    #[test]
    fn no_session_scope_passes_through() {
        let cache = InstructionCache::new();
        let input = CompressionInput {
            session_scope: None,
            instruction_cache: &cache,
        };
        let stage = DedupStage;
        let out = stage.process(&input, "some instructions");
        assert!(!out.modified);
        assert!(!out.short_circuit);
    }

    #[test]
    fn first_turn_passes_through() {
        let cache = InstructionCache::new();
        let input = CompressionInput {
            session_scope: Some("sess-1"),
            instruction_cache: &cache,
        };
        let stage = DedupStage;
        let out = stage.process(&input, "instruction content here");
        assert!(!out.modified);
        assert!(!out.short_circuit);
    }

    #[test]
    fn second_turn_dedup_replaces() {
        let cache = InstructionCache::new();
        let input = CompressionInput {
            session_scope: Some("sess-1"),
            instruction_cache: &cache,
        };
        let stage = DedupStage;

        let text = "Long instruction content. ".repeat(50);
        // Turn 1: registers hash
        let out1 = stage.process(&input, &text);
        assert!(!out1.modified);

        // Turn 2: dedup kicks in
        let out2 = stage.process(&input, &text);
        assert!(out2.modified);
        assert!(out2.short_circuit);
        assert!(out2.bytes_saved > 0);
        assert!(out2.text.contains("hash="));
        assert!(out2.text.contains("turn=2"));
    }

    #[test]
    fn changed_instructions_reset_dedup() {
        let cache = InstructionCache::new();
        let input = CompressionInput {
            session_scope: Some("sess-2"),
            instruction_cache: &cache,
        };
        let stage = DedupStage;

        let text_a = "Instructions version A. ".repeat(20);
        let text_b = "Instructions version B. ".repeat(20);

        // Turn 1: register A
        stage.process(&input, &text_a);
        // Turn 2: different text B — should register, not dedup
        let out = stage.process(&input, &text_b);
        assert!(!out.modified);

        // Turn 3: B again — now dedup
        let out = stage.process(&input, &text_b);
        assert!(out.modified);
        assert!(out.short_circuit);
    }
}
