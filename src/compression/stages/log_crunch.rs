// ═════════════════════════════════════════════════════════════════════
// Stage 3 — LogCrunchStage (static minification)
// ═════════════════════════════════════════════════════════════════════
//
// Strips low-signal content from instruction text:
//  - HTML/XML comments (single-line and multi-line)
//  - Decorative horizontal rules (---, ***, ___, ═══, ───, etc.)
//  - Consecutive blank lines (collapsed to one)
//  - Unicode box-drawing decoration lines

use crate::compression::pipeline::{CompressionInput, CompressionStage, StageOutput};

/// Static minification stage that strips decoration and comments.
pub struct LogCrunchStage;

impl LogCrunchStage {
    /// Minify text, returning (minified, bytes_saved).
    ///
    /// This is also available as a free function for use outside the
    /// pipeline (backward compat with `context_compress::minify_instructions`).
    pub fn minify(text: &str) -> (String, usize) {
        let original_len = text.len();
        let mut lines: Vec<&str> = Vec::new();
        let mut in_comment_block = false;

        for line in text.lines() {
            let trimmed = line.trim();

            // ── Multi-line HTML/XML comment blocks ────────────────
            if !in_comment_block && trimmed.starts_with("<!--") && !trimmed.ends_with("-->") {
                in_comment_block = true;
                continue;
            }
            if in_comment_block {
                if trimmed.contains("-->") {
                    in_comment_block = false;
                }
                continue;
            }

            // ── Single-line HTML/XML comments ─────────────────────
            if trimmed.starts_with("<!--") && trimmed.ends_with("-->") {
                continue;
            }

            // ── Decorative markdown rules ─────────────────────────
            if trimmed == "---" || trimmed == "***" || trimmed == "___" {
                continue;
            }

            // ── Consecutive blank lines → collapse to one ─────────
            if trimmed.is_empty()
                && let Some(prev) = lines.last()
                && prev.trim().is_empty()
            {
                continue;
            }

            // ── Unicode box-drawing / decoration lines ────────────
            if trimmed.len() > 3
                && trimmed
                    .chars()
                    .all(|c| matches!(c, '═' | '─' | '━' | '=' | '-'))
            {
                continue;
            }

            lines.push(line);
        }

        let result = lines.join("\n");
        let saved = original_len.saturating_sub(result.len());
        (result, saved)
    }
}

impl CompressionStage for LogCrunchStage {
    fn name(&self) -> &'static str {
        "log_crunch"
    }

    fn process(&self, _input: &CompressionInput, text: &str) -> StageOutput {
        let (minified, saved) = Self::minify(text);
        if saved > 0 {
            StageOutput {
                text: minified,
                modified: true,
                bytes_saved: saved,
                short_circuit: false,
            }
        } else {
            StageOutput::unchanged(text)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compression::cache::InstructionCache;

    #[test]
    fn strips_single_line_comments() {
        let (result, saved) = LogCrunchStage::minify("before\n<!-- comment -->\nafter");
        assert!(saved > 0);
        assert!(!result.contains("<!-- comment -->"));
        assert!(result.contains("before"));
        assert!(result.contains("after"));
    }

    #[test]
    fn strips_multiline_comments() {
        let input = "before\n<!-- start\nmiddle\nend -->\nafter";
        let (result, saved) = LogCrunchStage::minify(input);
        assert!(saved > 0);
        assert!(!result.contains("start"));
        assert!(!result.contains("middle"));
        assert!(result.contains("after"));
    }

    #[test]
    fn strips_decorative_rules() {
        let input = "Title\n---\nContent\n***\nMore\n___\nEnd";
        let (result, saved) = LogCrunchStage::minify(input);
        assert!(saved > 0);
        assert!(!result.contains("\n---\n"));
        assert!(!result.contains("\n***\n"));
        assert!(!result.contains("\n___\n"));
    }

    #[test]
    fn collapses_blank_lines() {
        let input = "Line 1\n\n\n\n\nLine 2";
        let (result, _saved) = LogCrunchStage::minify(input);
        // Should have at most one blank line between content
        assert!(!result.contains("\n\n\n"));
    }

    #[test]
    fn strips_box_drawing() {
        let input = "Title\n═══════════════════════════════\nContent";
        let (result, saved) = LogCrunchStage::minify(input);
        assert!(saved > 0);
        assert!(!result.contains("═══"));
    }

    #[test]
    fn preserves_meaningful_content() {
        let input = "# Instructions\n\nDo the thing.\n\n## Details\n\nMore info.";
        let (result, _saved) = LogCrunchStage::minify(input);
        assert!(result.contains("# Instructions"));
        assert!(result.contains("Do the thing."));
        assert!(result.contains("## Details"));
    }

    #[test]
    fn stage_noop_on_clean_text() {
        let cache = InstructionCache::new();
        let input = CompressionInput {
            session_scope: None,
            instruction_cache: &cache,
        };
        let stage = LogCrunchStage;
        let out = stage.process(&input, "No decoration here");
        assert!(!out.modified);
        assert_eq!(out.bytes_saved, 0);
    }
}
