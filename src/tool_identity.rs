//! AI-tool identification from HTTP User-Agent headers.
//!
//! Maps well-known User-Agent prefixes to a canonical tool name so that
//! Isartor can track per-tool metrics (requests, cache hits, cost savings).
//!
//! The mapping is deliberately conservative: if no known prefix matches,
//! `"unknown"` is returned rather than echoing the raw header.

/// Known tool patterns matched against the lowercased User-Agent header.
/// Each entry is `(prefix, canonical_tool_name)`.
const KNOWN_TOOLS: &[(&str, &str)] = &[
    // Anthropic
    ("claude-code", "claude-code"),
    ("claude-cli", "claude-code"),
    ("claudecode", "claude-code"),
    // GitHub Copilot
    ("copilot", "copilot"),
    ("github-copilot", "copilot"),
    ("githubcopilot", "copilot"),
    // Cursor IDE
    ("cursor", "cursor"),
    // OpenAI Codex
    ("codex", "codex"),
    ("openai-codex", "codex"),
    // Gemini CLI
    ("gemini-cli", "gemini-cli"),
    ("gemini_cli", "gemini-cli"),
    // OpenClaw
    ("openclaw", "openclaw"),
    // Windsurf
    ("windsurf", "windsurf"),
    // Zed
    ("zed", "zed"),
    // Cline / Roo Code
    ("cline", "cline"),
    ("roo-code", "roo-code"),
    ("roocode", "roo-code"),
    // aider
    ("aider", "aider"),
    // Continue.dev
    ("continue", "continue"),
    // Generic curl / httpie (useful for debugging)
    ("curl", "curl"),
    ("httpie", "httpie"),
];

/// Identify the AI tool from a raw `User-Agent` header value.
///
/// Returns the canonical tool name if a known prefix matches (case-insensitive),
/// or `"unknown"` otherwise.
pub fn identify_tool(user_agent: &str) -> &'static str {
    let ua_lower = user_agent.to_ascii_lowercase();
    for &(prefix, tool) in KNOWN_TOOLS {
        if ua_lower.starts_with(prefix) || ua_lower.contains(prefix) {
            return tool;
        }
    }
    "unknown"
}

/// Identify tool from an optional User-Agent, with a fallback based on
/// the traffic surface (e.g. MCP → "copilot").
pub fn identify_tool_or_fallback(user_agent: Option<&str>, traffic_surface: &str) -> &'static str {
    if let Some(ua) = user_agent {
        let tool = identify_tool(ua);
        if tool != "unknown" {
            return tool;
        }
    }
    // Fallback: infer from traffic surface.
    match traffic_surface {
        "mcp" => "copilot",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_user_agents_are_identified() {
        assert_eq!(identify_tool("claude-code/1.0.0"), "claude-code");
        assert_eq!(identify_tool("Claude-CLI/2.3"), "claude-code");
        assert_eq!(identify_tool("GitHub-Copilot/1.0"), "copilot");
        assert_eq!(identify_tool("cursor/0.44"), "cursor");
        assert_eq!(identify_tool("codex/1.0"), "codex");
        assert_eq!(identify_tool("gemini-cli/0.1"), "gemini-cli");
        assert_eq!(identify_tool("curl/8.4.0"), "curl");
        assert_eq!(identify_tool("Windsurf/1.0"), "windsurf");
    }

    #[test]
    fn unknown_user_agents_return_unknown() {
        assert_eq!(identify_tool("Mozilla/5.0"), "unknown");
        assert_eq!(identify_tool("SomeRandomTool/1.0"), "unknown");
        assert_eq!(identify_tool(""), "unknown");
    }

    #[test]
    fn fallback_uses_traffic_surface() {
        assert_eq!(identify_tool_or_fallback(None, "mcp"), "copilot");
        assert_eq!(identify_tool_or_fallback(None, "gateway"), "unknown");
        assert_eq!(
            identify_tool_or_fallback(Some("cursor/0.44"), "gateway"),
            "cursor"
        );
        assert_eq!(
            identify_tool_or_fallback(Some("Mozilla/5.0"), "mcp"),
            "copilot"
        );
    }
}
