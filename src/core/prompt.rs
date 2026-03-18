use serde_json::Value;

/// Extract a stable "prompt string" from various client request formats.
///
/// Supported inputs:
/// - Isartor native: {"prompt": "..."}
/// - OpenAI Chat Completions: {"messages": [{"role": "user", "content": "..."}, ...]}
/// - Anthropic Messages: {"system": "...", "messages": [{"role": "user", "content": "..."|[{"type":"text","text":"..."}, ...]}, ...]}
///
/// Falls back to treating the body as UTF-8.
pub fn extract_prompt(body: &[u8]) -> String {
    let Ok(v) = serde_json::from_slice::<Value>(body) else {
        return String::from_utf8_lossy(body).to_string();
    };

    // 1) Native format: {"prompt": "..."}
    if let Some(p) = v.get("prompt").and_then(|p| p.as_str()) {
        return p.to_string();
    }

    // 2) Chat-like format: {"messages": [...]}
    if let Some(messages) = v.get("messages").and_then(|m| m.as_array()) {
        let mut parts: Vec<String> = Vec::with_capacity(messages.len() + 1);

        // Anthropic supports a top-level system field.
        if let Some(system) = v.get("system").and_then(|s| s.as_str())
            && !system.trim().is_empty()
        {
            parts.push(format!("system: {system}"));
        }

        for msg in messages {
            let role = msg
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or("unknown");

            let content = match msg.get("content") {
                Some(Value::String(s)) => s.clone(),
                // Anthropic: content is an array of blocks.
                Some(Value::Array(blocks)) => {
                    let mut buf = String::new();
                    for block in blocks {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            if !buf.is_empty() {
                                buf.push('\n');
                            }
                            buf.push_str(text);
                        }
                    }
                    buf
                }
                Some(other) => other.to_string(),
                None => "".to_string(),
            };

            // Skip empty messages to avoid creating accidental identical prompts.
            if content.trim().is_empty() {
                continue;
            }

            parts.push(format!("{role}: {content}"));
        }

        if !parts.is_empty() {
            return parts.join("\n");
        }
    }

    // 3) Unknown JSON: use the raw JSON string for cache stability.
    v.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_native_prompt() {
        let body = br#"{"prompt":"hello"}"#;
        assert_eq!(extract_prompt(body), "hello");
    }

    #[test]
    fn extracts_openai_messages() {
        let body = br#"{"model":"gpt","messages":[{"role":"system","content":"be brief"},{"role":"user","content":"2+2?"}]}"#;
        let p = extract_prompt(body);
        assert!(p.contains("system: be brief"));
        assert!(p.contains("user: 2+2?"));
    }

    #[test]
    fn extracts_anthropic_blocks() {
        let body = br#"{"system":"hi","messages":[{"role":"user","content":[{"type":"text","text":"hello"}]}]}"#;
        let p = extract_prompt(body);
        assert!(p.contains("system: hi"));
        assert!(p.contains("user: hello"));
    }
}
