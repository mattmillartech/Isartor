// ═════════════════════════════════════════════════════════════════════
// Request Body Optimizer — JSON-level rewriting for L2.5.
// ═════════════════════════════════════════════════════════════════════
//
// Extracts system/instruction messages from OpenAI, Anthropic, and
// native request bodies, runs them through the CompressionPipeline,
// and reassembles the JSON with compressed content.

use bytes::Bytes;
use serde_json::Value;

use super::InstructionCache;
use super::pipeline::{CompressionInput, CompressionPipeline};
use super::stages::{ContentClassifier, DedupStage, LogCrunchStage};

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

/// Build the default compression pipeline based on config flags.
pub fn build_pipeline(enable_dedup: bool, enable_minify: bool) -> CompressionPipeline {
    let mut pipeline = CompressionPipeline::new().add_stage(Box::new(ContentClassifier));

    if enable_dedup {
        pipeline = pipeline.add_stage(Box::new(DedupStage));
    }
    if enable_minify {
        pipeline = pipeline.add_stage(Box::new(LogCrunchStage));
    }
    pipeline
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
    fn optimize_anthropic_system() {
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
    fn optimize_dedup_across_turns() {
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

        let r1 = optimize_request_body(&body_bytes, Some("sess1"), &cache, true, true);
        assert!(r1.modified, "Turn 1 should modify (log_crunch)");
        assert!(r1.strategy.contains("log_crunch"));

        let r2 = optimize_request_body(&body_bytes, Some("sess1"), &cache, true, true);
        assert!(r2.modified, "Turn 2 should modify (dedup)");
        assert!(r2.strategy.contains("dedup"));
        assert!(r2.bytes_saved > r1.bytes_saved);
    }

    #[test]
    fn conversational_content_skipped() {
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
    fn pipeline_with_system_messages_array() {
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

    // ── Malformed / edge-case inputs ────────────────────────────

    #[test]
    fn malformed_json_returns_unmodified() {
        let cache = InstructionCache::new();
        let garbage = b"this is not json {{{";

        let result = optimize_request_body(garbage, None, &cache, true, true);
        assert!(!result.modified);
        assert_eq!(result.bytes_saved, 0);
        assert_eq!(result.strategy, "none");
        assert_eq!(result.body.as_ref(), garbage);
    }

    #[test]
    fn empty_body_returns_unmodified() {
        let cache = InstructionCache::new();

        let result = optimize_request_body(b"", None, &cache, true, true);
        assert!(!result.modified);
        assert_eq!(result.body.as_ref(), b"");
    }

    #[test]
    fn json_without_system_or_messages_unmodified() {
        let cache = InstructionCache::new();
        let body = serde_json::json!({
            "model": "gpt-4.1",
            "prompt": "Just a prompt"
        });
        let body_bytes = serde_json::to_vec(&body).unwrap();

        let result = optimize_request_body(&body_bytes, None, &cache, true, true);
        assert!(!result.modified);
    }

    #[test]
    fn user_messages_not_modified() {
        let cache = InstructionCache::new();
        let long_user_msg = "Tell me about ".to_string() + &"topic ".repeat(200);
        let body = serde_json::json!({
            "model": "gpt-4.1",
            "messages": [
                {"role": "user", "content": long_user_msg}
            ]
        });
        let body_bytes = serde_json::to_vec(&body).unwrap();

        let result = optimize_request_body(&body_bytes, None, &cache, true, true);
        assert!(!result.modified, "User messages should never be modified");
    }

    // ── Anthropic array system field ────────────────────────────

    #[test]
    fn anthropic_system_array_format() {
        let cache = InstructionCache::new();
        let long_text =
            "You are an AI assistant. <custom_instructions>Rules here.</custom_instructions>\n\
            <!-- tracking -->\n---\n═══\n\n\nDo things.\n"
                .to_string()
                + &"padding ".repeat(50);
        let body = serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "system": [
                {"type": "text", "text": long_text}
            ],
            "messages": [{"role": "user", "content": "Hi"}]
        });
        let body_bytes = serde_json::to_vec(&body).unwrap();

        let result = optimize_request_body(&body_bytes, None, &cache, false, true);
        assert!(result.modified);

        // Verify the result is still an array format
        let doc: Value = serde_json::from_slice(&result.body).unwrap();
        assert!(
            doc["system"].is_array(),
            "Anthropic system should remain array format"
        );
        assert!(doc["system"][0]["type"].as_str() == Some("text"));
    }

    // ── Pipeline config variations ──────────────────────────────

    #[test]
    fn dedup_disabled_only_minifies() {
        let cache = InstructionCache::new();
        let instructions = "You are an AI assistant. <custom_instructions>Long instructions.</custom_instructions>\n\
            <!-- comment -->\n---\n═══\n\nContent.\n".to_string() + &"padding ".repeat(50);
        let body = serde_json::json!({
            "model": "gpt-4.1",
            "system": &instructions,
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let body_bytes = serde_json::to_vec(&body).unwrap();

        // Turn 1
        let r1 = optimize_request_body(&body_bytes, Some("sess-no-dedup"), &cache, false, true);
        assert!(r1.modified);
        assert!(r1.strategy.contains("log_crunch"));

        // Turn 2: no dedup since disabled
        let r2 = optimize_request_body(&body_bytes, Some("sess-no-dedup"), &cache, false, true);
        assert!(r2.modified);
        assert!(!r2.strategy.contains("dedup"), "Dedup should be disabled");
    }

    #[test]
    fn minify_disabled_only_dedups() {
        let cache = InstructionCache::new();
        let instructions = "You are an AI assistant. <custom_instructions>Long instructions.</custom_instructions>\n\
            <!-- comment -->\n---\n═══\n\nContent.\n".to_string() + &"padding ".repeat(50);
        let body = serde_json::json!({
            "model": "gpt-4.1",
            "system": &instructions,
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let body_bytes = serde_json::to_vec(&body).unwrap();

        // Turn 1: register (no minify, no dedup on first pass)
        let _r1 = optimize_request_body(&body_bytes, Some("sess-no-minify"), &cache, true, false);
        // Content classifier may pass but log_crunch is disabled, so it depends on content
        // Turn 2: should dedup
        let r2 = optimize_request_body(&body_bytes, Some("sess-no-minify"), &cache, true, false);
        assert!(r2.modified);
        assert!(r2.strategy.contains("dedup"));
        assert!(!r2.strategy.contains("log_crunch"));
    }

    #[test]
    fn both_disabled_no_modification() {
        let cache = InstructionCache::new();
        let instructions =
            "You are an AI assistant. <custom_instructions>Instructions.</custom_instructions>\n\
            <!-- comment -->\nContent.\n"
                .to_string()
                + &"padding ".repeat(50);
        let body = serde_json::json!({
            "model": "gpt-4.1",
            "system": &instructions,
            "messages": [{"role": "user", "content": "Hello"}]
        });
        let body_bytes = serde_json::to_vec(&body).unwrap();

        // Content classifier is always present but only gates; without dedup/minify no changes
        let result = optimize_request_body(&body_bytes, None, &cache, false, false);
        assert!(!result.modified);
        assert_eq!(result.strategy, "none");
    }

    // ── build_pipeline ──────────────────────────────────────────

    #[test]
    fn build_pipeline_all_enabled() {
        let p = build_pipeline(true, true);
        assert_eq!(p.len(), 3); // classifier + dedup + log_crunch
    }

    #[test]
    fn build_pipeline_none_enabled() {
        let p = build_pipeline(false, false);
        assert_eq!(p.len(), 1); // only classifier
    }

    #[test]
    fn build_pipeline_dedup_only() {
        let p = build_pipeline(true, false);
        assert_eq!(p.len(), 2); // classifier + dedup
    }

    #[test]
    fn build_pipeline_minify_only() {
        let p = build_pipeline(false, true);
        assert_eq!(p.len(), 2); // classifier + log_crunch
    }

    // ── extract_text_from_value ─────────────────────────────────

    #[test]
    fn extract_text_from_string_value() {
        let val = Value::String("hello".into());
        assert_eq!(extract_text_from_value(&val), Some("hello".into()));
    }

    #[test]
    fn extract_text_from_content_blocks() {
        let val = serde_json::json!([
            {"type": "text", "text": "first"},
            {"type": "text", "text": "second"}
        ]);
        assert_eq!(extract_text_from_value(&val), Some("first\nsecond".into()));
    }

    #[test]
    fn extract_text_from_empty_array() {
        let val = serde_json::json!([]);
        assert_eq!(extract_text_from_value(&val), None);
    }

    #[test]
    fn extract_text_from_non_text_blocks() {
        let val = serde_json::json!([
            {"type": "image", "url": "http://example.com/img.png"}
        ]);
        assert_eq!(extract_text_from_value(&val), None);
    }

    #[test]
    fn extract_text_from_null() {
        assert_eq!(extract_text_from_value(&Value::Null), None);
    }

    #[test]
    fn extract_text_from_number() {
        let val = serde_json::json!(42);
        assert_eq!(extract_text_from_value(&val), None);
    }
}
