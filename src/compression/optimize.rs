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
}
