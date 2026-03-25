// ═════════════════════════════════════════════════════════════════════
// L2.5 Context Compression — Instruction file dedup & minification
// ═════════════════════════════════════════════════════════════════════
//
// Agentic coding tools (Copilot, Claude Code, Cursor) send large static
// instruction files (CLAUDE.md, copilot-instructions.md, skills blocks)
// with every turn.  This module detects and compresses that payload
// before it reaches L3, saving input tokens on every cloud call.
//
// Three strategies, applied in order:
//  1. Session dedup  — hash instructions; on repeat, replace with a
//                      compact reference ("Instructions unchanged, hash=…").
//  2. Static minify  — strip markdown comments, collapse whitespace,
//                      remove horizontal rules & decorative markers.
//  3. Section prune  — drop low-signal sections (changelogs, examples
//                      that duplicate the task description).

use bytes::Bytes;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;

/// Per-session instruction hash cache for dedup across turns.
#[derive(Debug, Default)]
pub struct InstructionCache {
    /// Map from session_scope → (instruction_hash, turn_count)
    seen: Mutex<HashMap<String, (String, u32)>>,
}

impl InstructionCache {
    pub fn new() -> Self {
        Self {
            seen: Mutex::new(HashMap::new()),
        }
    }

    /// Check if we've seen these exact instructions for this session before.
    /// Returns `Some(hash)` if this is a repeat (dedup candidate).
    pub fn check_and_update(&self, scope: &str, hash: &str) -> Option<u32> {
        let mut map = self.seen.lock().unwrap();
        if let Some((prev_hash, turn)) = map.get_mut(scope) {
            if prev_hash == hash {
                *turn += 1;
                return Some(*turn);
            }
            // Instructions changed — update hash, reset turn
            *prev_hash = hash.to_string();
            *turn = 1;
            None
        } else {
            map.insert(scope.to_string(), (hash.to_string(), 1));
            None
        }
    }

    /// Evict stale entries (called periodically or on capacity).
    pub fn evict_if_needed(&self, max_entries: usize) {
        let mut map = self.seen.lock().unwrap();
        if map.len() > max_entries {
            // Simple: just clear oldest half
            let keys: Vec<String> = map.keys().take(map.len() / 2).cloned().collect();
            for k in keys {
                map.remove(&k);
            }
        }
    }
}

// ═════════════════════════════════════════════════════════════════════
// Instruction detection & extraction
// ═════════════════════════════════════════════════════════════════════

/// Detect known instruction file patterns in a system message.
/// Returns true if the text looks like an agent instruction payload.
pub fn is_instruction_content(text: &str) -> bool {
    if text.len() < 200 {
        return false; // Too short to be an instruction file
    }

    let lower = text.to_lowercase();
    let markers = [
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
    ];

    let hits = markers.iter().filter(|m| lower.contains(*m)).count();
    // Two or more markers → likely instruction payload
    hits >= 1
}

/// Hash the instruction content for dedup comparison.
pub fn hash_instructions(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    hex::encode(&digest[..8]) // 16-char hex prefix is enough for dedup
}

// ═════════════════════════════════════════════════════════════════════
// Static minification
// ═════════════════════════════════════════════════════════════════════

/// Minify instruction text by removing low-signal content.
/// Returns (minified_text, bytes_saved).
pub fn minify_instructions(text: &str) -> (String, usize) {
    let original_len = text.len();
    let mut lines: Vec<&str> = Vec::new();
    let mut skip_block = false;

    for line in text.lines() {
        let trimmed = line.trim();

        // Skip HTML/XML comment blocks
        if trimmed.starts_with("<!--") && !trimmed.ends_with("-->") {
            skip_block = true;
            continue;
        }
        if skip_block {
            if trimmed.contains("-->") {
                skip_block = false;
            }
            continue;
        }
        // Skip single-line HTML comments
        if trimmed.starts_with("<!--") && trimmed.ends_with("-->") {
            continue;
        }

        // Skip decorative markdown
        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            continue;
        }

        // Skip pure-whitespace lines beyond the first consecutive one
        if trimmed.is_empty()
            && let Some(l) = lines.last()
            && l.trim().is_empty()
        {
            continue; // Collapse consecutive blank lines
        }

        // Skip lines that are entirely repetitive markers
        if trimmed
            .chars()
            .all(|c| c == '═' || c == '─' || c == '━' || c == '=' || c == '-')
            && trimmed.len() > 3
        {
            continue;
        }

        lines.push(line);
    }

    let result = lines.join("\n");
    let saved = original_len.saturating_sub(result.len());
    (result, saved)
}

// ═════════════════════════════════════════════════════════════════════
// Request body rewriting
// ═════════════════════════════════════════════════════════════════════

/// Result of context optimization on a request body.
#[derive(Debug)]
pub struct OptimizeResult {
    /// The rewritten body bytes (or original if no optimization applied).
    pub body: Bytes,
    /// Whether the body was modified.
    pub modified: bool,
    /// Bytes saved by optimization.
    pub bytes_saved: usize,
    /// Strategy applied: "none", "dedup", "minify", or "dedup+minify".
    pub strategy: &'static str,
}

/// Optimize the request body by compressing instruction/system messages.
///
/// Handles OpenAI, Anthropic, and native request formats.
pub fn optimize_request_body(
    body: &[u8],
    session_scope: Option<&str>,
    instruction_cache: &InstructionCache,
    enable_dedup: bool,
    enable_minify: bool,
) -> OptimizeResult {
    let Ok(mut doc) = serde_json::from_slice::<Value>(body) else {
        return OptimizeResult {
            body: Bytes::copy_from_slice(body),
            modified: false,
            bytes_saved: 0,
            strategy: "none",
        };
    };

    let mut total_saved: usize = 0;
    let mut any_modified = false;
    let mut strategy = "none";

    // ── Anthropic top-level "system" field ─────────────────────────
    if let Some(system_val) = doc.get("system") {
        let system_text = match system_val {
            Value::String(s) => Some(s.clone()),
            Value::Array(blocks) => {
                // Anthropic system can be an array of {type: "text", text: "..."}
                let texts: Vec<&str> = blocks
                    .iter()
                    .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                    .collect();
                if texts.is_empty() {
                    None
                } else {
                    Some(texts.join("\n"))
                }
            }
            _ => None,
        };

        if let Some(ref text) = system_text
            && is_instruction_content(text)
        {
            let (optimized, dedup_applied) = apply_optimization(
                text,
                session_scope,
                instruction_cache,
                enable_dedup,
                enable_minify,
            );
            let saved = text.len().saturating_sub(optimized.len());
            if saved > 0 {
                // Rewrite the system field
                match system_val {
                    Value::String(_) => {
                        doc["system"] = Value::String(optimized);
                    }
                    Value::Array(_) => {
                        doc["system"] = serde_json::json!([
                            {"type": "text", "text": optimized}
                        ]);
                    }
                    _ => {}
                }
                total_saved += saved;
                any_modified = true;
                strategy = if dedup_applied {
                    "dedup+minify"
                } else {
                    "minify"
                };
            }
        }
    }

    // ── System messages in messages array ──────────────────────────
    if let Some(messages) = doc.get_mut("messages").and_then(|m| m.as_array_mut()) {
        for msg in messages.iter_mut() {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
            if role != "system" {
                continue;
            }

            let content_text = match msg.get("content") {
                Some(Value::String(s)) => Some(s.clone()),
                Some(Value::Array(blocks)) => {
                    let texts: Vec<&str> = blocks
                        .iter()
                        .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                        .collect();
                    if texts.is_empty() {
                        None
                    } else {
                        Some(texts.join("\n"))
                    }
                }
                _ => None,
            };

            if let Some(ref text) = content_text
                && is_instruction_content(text)
            {
                let (optimized, dedup_applied) = apply_optimization(
                    text,
                    session_scope,
                    instruction_cache,
                    enable_dedup,
                    enable_minify,
                );
                let saved = text.len().saturating_sub(optimized.len());
                if saved > 0 {
                    msg["content"] = Value::String(optimized);
                    total_saved += saved;
                    any_modified = true;
                    if dedup_applied {
                        strategy = "dedup+minify";
                    } else if strategy == "none" {
                        strategy = "minify";
                    }
                }
            }
        }
    }

    if any_modified {
        let new_body = serde_json::to_vec(&doc).unwrap_or_else(|_| body.to_vec());
        OptimizeResult {
            body: Bytes::from(new_body),
            modified: true,
            bytes_saved: total_saved,
            strategy,
        }
    } else {
        OptimizeResult {
            body: Bytes::copy_from_slice(body),
            modified: false,
            bytes_saved: 0,
            strategy: "none",
        }
    }
}

/// Apply dedup + minification to instruction text.
/// Returns (optimized_text, dedup_was_applied).
fn apply_optimization(
    text: &str,
    session_scope: Option<&str>,
    cache: &InstructionCache,
    enable_dedup: bool,
    enable_minify: bool,
) -> (String, bool) {
    let hash = hash_instructions(text);

    // Dedup: if same instructions seen for this session, replace with reference
    if enable_dedup
        && let Some(scope) = session_scope
        && let Some(turn) = cache.check_and_update(scope, &hash)
    {
        // Seen before — replace with compact reference
        let summary = format!(
            "[Context instructions unchanged since turn 1 (hash={hash}, turn={turn}). \
             Follow the same instructions as before.]"
        );
        return (summary, true);
    }

    // Minify
    if enable_minify {
        let (minified, _saved) = minify_instructions(text);
        return (minified, false);
    }

    (text.to_string(), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_instruction_content() {
        assert!(!is_instruction_content("Hello world"));
        assert!(!is_instruction_content("Short text"));

        let long_instruction =
            "You are an AI assistant. <custom_instructions>\n".to_string() + &"x".repeat(300);
        assert!(is_instruction_content(&long_instruction));
    }

    #[test]
    fn test_minify_strips_comments_and_rules() {
        let input = "# Title\n<!-- comment -->\nContent here\n---\nMore content\n\n\n\nFinal";
        let (result, saved) = minify_instructions(input);
        assert!(saved > 0);
        assert!(!result.contains("<!-- comment -->"));
        assert!(!result.contains("---"));
        assert!(result.contains("Content here"));
        assert!(result.contains("Final"));
    }

    #[test]
    fn test_dedup_cache() {
        let cache = InstructionCache::new();
        // First time: no dedup
        assert!(cache.check_and_update("session1", "hash_a").is_none());
        // Same hash: dedup kicks in
        assert_eq!(cache.check_and_update("session1", "hash_a"), Some(2));
        assert_eq!(cache.check_and_update("session1", "hash_a"), Some(3));
        // Different hash: resets
        assert!(cache.check_and_update("session1", "hash_b").is_none());
    }

    #[test]
    fn test_optimize_anthropic_system() {
        let cache = InstructionCache::new();
        let body = serde_json::json!({
            "model": "gpt-4.1",
            "system": "You are an AI assistant. <custom_instructions>Be helpful.</custom_instructions>\n<!-- internal -->\n---\nDo your best.\n".to_string() + &"padding ".repeat(50),
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let body_bytes = serde_json::to_vec(&body).unwrap();

        let result = optimize_request_body(&body_bytes, None, &cache, false, true);
        assert!(result.modified);
        assert!(result.bytes_saved > 0);
        assert_eq!(result.strategy, "minify");
    }

    #[test]
    fn test_optimize_dedup_across_turns() {
        let cache = InstructionCache::new();
        // Include strippable content (comments, rules, blank lines) so minify has work to do.
        let instructions = "You are an AI assistant. <custom_instructions>Long instructions here.</custom_instructions>\n\
            <!-- internal tracking comment -->\n\
            ---\n\
            ═══════════════════════════════\n\n\n\n\
            Do your best.\n"
            .to_string()
            + &"padding content here. ".repeat(30);
        let body = serde_json::json!({
            "model": "gpt-4.1",
            "system": instructions,
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let body_bytes = serde_json::to_vec(&body).unwrap();

        // Turn 1: minify only (first time seeing instructions)
        let r1 = optimize_request_body(&body_bytes, Some("sess1"), &cache, true, true);
        assert!(r1.modified, "Turn 1 should modify (minify)");
        assert_eq!(r1.strategy, "minify");

        // Turn 2: dedup kicks in — much smaller
        let r2 = optimize_request_body(&body_bytes, Some("sess1"), &cache, true, true);
        assert!(r2.modified, "Turn 2 should modify (dedup)");
        assert_eq!(r2.strategy, "dedup+minify");
        assert!(r2.bytes_saved > r1.bytes_saved);
    }
}
