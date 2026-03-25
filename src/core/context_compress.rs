// ═════════════════════════════════════════════════════════════════════
// L2.5 Context Compression — Instruction file dedup & minification
// ═════════════════════════════════════════════════════════════════════
//
// Agentic coding tools (Copilot, Claude Code, Cursor) send large static
// instruction files (CLAUDE.md, copilot-instructions.md, skills blocks)
// with every turn.  This module detects and compresses that payload
// before it reaches L3, saving input tokens on every cloud call.
//
// The heavy lifting is now delegated to the `CompressionPipeline`
// in `src/compression/`.  This module provides the request-body
// rewriting logic that extracts system messages, runs them through
// the pipeline, and reassembles the JSON body.

use bytes::Bytes;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;

use crate::compression::pipeline::{CompressionInput, CompressionPipeline};
use crate::compression::stages::{ContentClassifier, DedupStage, LogCrunchStage};

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
    /// Returns `Some(turn)` if this is a repeat (dedup candidate).
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
            let keys: Vec<String> = map.keys().take(map.len() / 2).cloned().collect();
            for k in keys {
                map.remove(&k);
            }
        }
    }
}

/// Hash the instruction content for dedup comparison.
pub fn hash_instructions(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    hex::encode(&digest[..8]) // 16-char hex prefix is enough for dedup
}

// ═════════════════════════════════════════════════════════════════════
// Pipeline construction
// ═════════════════════════════════════════════════════════════════════

/// Build the default compression pipeline based on config flags.
pub fn build_pipeline(enable_dedup: bool, enable_minify: bool) -> CompressionPipeline {
    let mut pipeline = CompressionPipeline::new()
        // Stage 1: gate — skip non-instruction content
        .add_stage(Box::new(ContentClassifier));

    // Stage 2: dedup (if enabled)
    if enable_dedup {
        pipeline = pipeline.add_stage(Box::new(DedupStage));
    }

    // Stage 3: static minification (if enabled)
    if enable_minify {
        pipeline = pipeline.add_stage(Box::new(LogCrunchStage));
    }

    pipeline
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
    /// Strategy applied (e.g. "none", "dedup", "log_crunch", "dedup+log_crunch").
    pub strategy: String,
}

/// Optimize the request body by compressing instruction/system messages.
///
/// Handles OpenAI, Anthropic, and native request formats.
/// Delegates to the `CompressionPipeline` for actual compression.
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
            strategy: "none".to_string(),
        };
    };

    let pipeline = build_pipeline(enable_dedup, enable_minify);
    let ctx = CompressionInput {
        session_scope,
        instruction_cache,
    };

    let mut total_saved: usize = 0;
    let mut any_modified = false;
    let mut strategy = String::from("none");

    // ── Anthropic top-level "system" field ─────────────────────────
    let system_is_array = doc.get("system").is_some_and(|v| v.is_array());

    if let Some(system_text) = doc.get("system").and_then(extract_text_from_value) {
        let output = pipeline.execute(&ctx, &system_text);
        if output.modified {
            if system_is_array {
                doc["system"] = serde_json::json!([
                    {"type": "text", "text": output.text}
                ]);
            } else {
                doc["system"] = Value::String(output.text);
            }
            total_saved += output.total_bytes_saved;
            any_modified = true;
            strategy = output.strategy;
        }
    }

    // ── System messages in messages array ──────────────────────────
    if let Some(messages) = doc.get_mut("messages").and_then(|m| m.as_array_mut()) {
        for msg in messages.iter_mut() {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
            if role != "system" {
                continue;
            }

            let content_text = extract_text_from_value(msg.get("content").unwrap_or(&Value::Null));

            if let Some(ref text) = content_text {
                let output = pipeline.execute(&ctx, text);
                if output.modified {
                    msg["content"] = Value::String(output.text);
                    total_saved += output.total_bytes_saved;
                    any_modified = true;
                    if strategy == "none" {
                        strategy = output.strategy;
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
            strategy: "none".to_string(),
        }
    }
}

/// Extract text from a JSON value (String, or Anthropic content-block array).
fn extract_text_from_value(val: &Value) -> Option<String> {
    match val {
        Value::String(s) => Some(s.clone()),
        Value::Array(blocks) => {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dedup_cache() {
        let cache = InstructionCache::new();
        assert!(cache.check_and_update("session1", "hash_a").is_none());
        assert_eq!(cache.check_and_update("session1", "hash_a"), Some(2));
        assert_eq!(cache.check_and_update("session1", "hash_a"), Some(3));
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
        assert!(result.strategy.contains("log_crunch"));
    }

    #[test]
    fn test_optimize_dedup_across_turns() {
        let cache = InstructionCache::new();
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
        assert!(r1.modified, "Turn 1 should modify (log_crunch)");
        assert!(r1.strategy.contains("log_crunch"));

        // Turn 2: dedup kicks in — much smaller
        let r2 = optimize_request_body(&body_bytes, Some("sess1"), &cache, true, true);
        assert!(r2.modified, "Turn 2 should modify (dedup)");
        assert!(r2.strategy.contains("dedup"));
        assert!(r2.bytes_saved > r1.bytes_saved);
    }

    #[test]
    fn test_conversational_content_skipped() {
        let cache = InstructionCache::new();
        let body = serde_json::json!({
            "model": "gpt-4.1",
            "messages": [
                {"role": "system", "content": "Be helpful"},
                {"role": "user", "content": "Hello"}
            ]
        });
        let body_bytes = serde_json::to_vec(&body).unwrap();

        let result = optimize_request_body(&body_bytes, None, &cache, true, true);
        assert!(
            !result.modified,
            "Short conversational system msg should not be compressed"
        );
    }

    #[test]
    fn test_pipeline_with_system_messages_array() {
        let cache = InstructionCache::new();
        let long_system =
            "You are an AI assistant. <custom_instructions>Instructions</custom_instructions>\n\
            <!-- comment -->\n---\n═══\n\n\nContent.\n"
                .to_string()
                + &"x ".repeat(200);
        let body = serde_json::json!({
            "model": "gpt-4.1",
            "messages": [
                {"role": "system", "content": long_system},
                {"role": "user", "content": "Hello"}
            ]
        });
        let body_bytes = serde_json::to_vec(&body).unwrap();

        let result = optimize_request_body(&body_bytes, None, &cache, false, true);
        assert!(result.modified);
        assert!(result.bytes_saved > 0);
    }
}
