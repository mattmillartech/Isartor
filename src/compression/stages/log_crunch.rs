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

    // ── Lossless: meaningful content must survive ────────────────

    #[test]
    fn preserves_code_blocks() {
        let input = "```rust\nfn main() {\n    println!(\"hello\");\n}\n```";
        let (result, _) = LogCrunchStage::minify(input);
        assert!(result.contains("fn main()"));
        assert!(result.contains("println!"));
    }

    #[test]
    fn preserves_markdown_headers() {
        let input = "# H1\n## H2\n### H3\nContent";
        let (result, _) = LogCrunchStage::minify(input);
        assert!(result.contains("# H1"));
        assert!(result.contains("## H2"));
        assert!(result.contains("### H3"));
    }

    #[test]
    fn preserves_bullet_lists() {
        let input = "- Item 1\n- Item 2\n  - Nested\n* Star item";
        let (result, _) = LogCrunchStage::minify(input);
        assert!(result.contains("- Item 1"));
        assert!(result.contains("- Item 2"));
        assert!(result.contains("* Star item"));
    }

    #[test]
    fn preserves_xml_tags_that_are_not_comments() {
        let input = "<custom_instructions>\nBe helpful.\n</custom_instructions>";
        let (result, _) = LogCrunchStage::minify(input);
        assert!(result.contains("<custom_instructions>"));
        assert!(result.contains("Be helpful."));
    }

    #[test]
    fn preserves_short_dashes_in_text() {
        // "---" is stripped as a rule, but "--" or "-" in text should survive
        let input = "Use the --flag option.\nOr use -f for short.";
        let (result, _) = LogCrunchStage::minify(input);
        assert!(result.contains("--flag"));
        assert!(result.contains("-f"));
    }

    #[test]
    fn preserves_equals_in_assignments() {
        // Short "=" should not be stripped (only long "===..." lines)
        let input = "x = 42\nlet result = compute();";
        let (result, _) = LogCrunchStage::minify(input);
        assert!(result.contains("x = 42"));
        assert!(result.contains("result = compute()"));
    }

    // ── Lossy: decoration correctly stripped ─────────────────────

    #[test]
    fn strips_mixed_decoration_types() {
        let input = "# Title\n════════════\nContent\n────────────\nMore\n━━━━━━━━━━━━\nEnd";
        let (result, saved) = LogCrunchStage::minify(input);
        assert!(saved > 0);
        assert!(!result.contains("════"));
        assert!(!result.contains("────"));
        assert!(!result.contains("━━━"));
        assert!(result.contains("# Title"));
        assert!(result.contains("Content"));
        assert!(result.contains("End"));
    }

    #[test]
    fn strips_long_equals_line() {
        let input = "Title\n============================\nContent";
        let (result, saved) = LogCrunchStage::minify(input);
        assert!(saved > 0);
        assert!(!result.contains("====="));
    }

    #[test]
    fn strips_long_dash_line() {
        let input = "Title\n----------------------------\nContent";
        let (result, saved) = LogCrunchStage::minify(input);
        assert!(saved > 0);
        assert!(!result.contains("----------"));
    }

    #[test]
    fn nested_html_comments() {
        // Multi-line comment with embedded markup
        let input = "Before\n<!-- \n<div>inner html</div>\nstill comment\n-->\nAfter";
        let (result, saved) = LogCrunchStage::minify(input);
        assert!(saved > 0);
        assert!(!result.contains("inner html"));
        assert!(result.contains("Before"));
        assert!(result.contains("After"));
    }

    #[test]
    fn comment_at_start_of_text() {
        let input = "<!-- top comment -->\nActual content here";
        let (result, saved) = LogCrunchStage::minify(input);
        assert!(saved > 0);
        assert!(!result.contains("top comment"));
        assert!(result.contains("Actual content"));
    }

    #[test]
    fn comment_at_end_of_text() {
        let input = "Content here\n<!-- trailing comment -->";
        let (result, saved) = LogCrunchStage::minify(input);
        assert!(saved > 0);
        assert!(!result.contains("trailing"));
    }

    // ── Edge cases ──────────────────────────────────────────────

    #[test]
    fn empty_input() {
        let (result, saved) = LogCrunchStage::minify("");
        assert_eq!(result, "");
        assert_eq!(saved, 0);
    }

    #[test]
    fn only_whitespace() {
        let (result, _) = LogCrunchStage::minify("\n\n\n\n\n");
        // Should collapse to at most one blank line
        assert!(result.len() < 5);
    }

    #[test]
    fn only_decoration() {
        let input = "---\n***\n___\n═══════\n───────\n━━━━━━━";
        let (result, saved) = LogCrunchStage::minify(input);
        assert!(saved > 0);
        assert!(result.trim().is_empty() || result.lines().count() <= 1);
    }

    #[test]
    fn idempotent_minification() {
        let input = "# Title\n<!-- comment -->\n---\nContent\n\n\n\nMore";
        let (first_pass, _) = LogCrunchStage::minify(input);
        let (second_pass, saved2) = LogCrunchStage::minify(&first_pass);
        assert_eq!(saved2, 0, "Second pass should be a no-op");
        assert_eq!(first_pass, second_pass);
    }

    #[test]
    fn real_world_claude_system_prompt_snippet() {
        let input = r#"<!-- isartor:copilot-instructions:start -->
# Copilot Instructions for Isartor

## Build, test, and lint commands

- Use `cargo build` for a local build
- Run the full test suite with `cargo test --all-features`

═══════════════════════════════════════════════════════════════════

## High-level architecture

---

- `src/main.rs` is the real boot sequence

***

Some important content here.
<!-- isartor:copilot-instructions:end -->"#;
        let (result, saved) = LogCrunchStage::minify(input);
        assert!(saved > 0);
        // Content preserved
        assert!(result.contains("# Copilot Instructions"));
        assert!(result.contains("cargo build"));
        assert!(result.contains("important content"));
        // Decoration removed
        assert!(!result.contains("═══"));
        assert!(!result.contains("<!-- isartor"));
    }
}
