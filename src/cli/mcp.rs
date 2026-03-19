//! Minimal MCP (Model Context Protocol) stdio server.
//!
//! Run with `isartor mcp` — Copilot CLI (or any MCP client) launches this as
//! a subprocess and communicates via JSON-RPC 2.0 over stdin/stdout.
//!
//! Exposed tools:
//! - `isartor_chat`: Send a prompt through Isartor's deflection stack and
//!   return the response. This enables L1a/L1b cache hits for Copilot.

use std::io::{self, BufRead, Write};

use clap::Parser;
use serde_json::{Value, json};

use super::connect::DEFAULT_GATEWAY_URL;

#[derive(Parser, Debug, Clone)]
pub struct McpArgs {
    /// Isartor gateway URL
    #[arg(long, default_value = DEFAULT_GATEWAY_URL, env = "ISARTOR_GATEWAY_URL")]
    pub gateway_url: String,

    /// Gateway API key
    #[arg(long, env = "ISARTOR__GATEWAY_API_KEY")]
    pub gateway_api_key: Option<String>,
}

/// Run the MCP stdio server (blocking — reads stdin line by line).
pub async fn handle_mcp(args: McpArgs) -> anyhow::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let msg: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue, // ignore non-JSON lines
        };

        // JSON-RPC notifications (no "id") don't require a response.
        let id = msg.get("id").cloned();
        let method = msg
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string();

        let response = match method.as_str() {
            "initialize" => handle_initialize(id),
            "notifications/initialized" | "initialized" => None, // notification, no response
            "tools/list" => handle_tools_list(id),
            "tools/call" => {
                let params = msg.get("params").cloned().unwrap_or(json!({}));
                Some(handle_tools_call(id, &params, &args).await)
            }
            "shutdown" => {
                if let Some(id) = id {
                    let _ = send(&mut stdout, &jsonrpc_ok(id, json!({})));
                }
                break;
            }
            _ => {
                // Unknown method → JSON-RPC error -32601 "Method not found"
                id.map(|id| jsonrpc_error(id, -32601, "Method not found"))
            }
        };

        if let Some(resp) = response {
            send(&mut stdout, &resp)?;
        }
    }

    Ok(())
}

fn handle_initialize(id: Option<Value>) -> Option<Value> {
    let id = id?;
    Some(jsonrpc_ok(
        id,
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "isartor",
                "version": env!("CARGO_PKG_VERSION")
            }
        }),
    ))
}

fn handle_tools_list(id: Option<Value>) -> Option<Value> {
    let id = id?;
    Some(jsonrpc_ok(
        id,
        json!({
            "tools": [
                {
                    "name": "isartor_chat",
                    "description": "Send a prompt through Isartor's deflection stack (L1a exact cache → L1b semantic cache → L2 SLM triage → L3 cloud). Returns the LLM response with cache/deflection metadata. Use this for any question or task that could benefit from cached responses.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "prompt": {
                                "type": "string",
                                "description": "The prompt or question to send"
                            },
                            "model": {
                                "type": "string",
                                "description": "Optional model name (e.g. gpt-4o-mini)"
                            }
                        },
                        "required": ["prompt"]
                    }
                }
            ]
        }),
    ))
}

async fn handle_tools_call(id: Option<Value>, params: &Value, args: &McpArgs) -> Value {
    let id = id.unwrap_or(Value::Null);
    let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");

    match tool_name {
        "isartor_chat" => {
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            let prompt = arguments
                .get("prompt")
                .and_then(|p| p.as_str())
                .unwrap_or("");

            if prompt.is_empty() {
                return jsonrpc_ok(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": "Error: prompt is required"
                        }],
                        "isError": true
                    }),
                );
            }

            match call_isartor_chat(&args.gateway_url, args.gateway_api_key.as_deref(), prompt)
                .await
            {
                Ok(result) => jsonrpc_ok(id, result),
                Err(e) => jsonrpc_ok(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": format!("Isartor error: {e}")
                        }],
                        "isError": true
                    }),
                ),
            }
        }
        _ => jsonrpc_error(id, -32602, &format!("Unknown tool: {tool_name}")),
    }
}

async fn call_isartor_chat(
    gateway_url: &str,
    api_key: Option<&str>,
    prompt: &str,
) -> anyhow::Result<Value> {
    let url = format!("{}/api/chat", gateway_url.trim_end_matches('/'));
    let client = reqwest::Client::new();

    let mut req = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&json!({ "prompt": prompt }));

    if let Some(key) = api_key {
        req = req.header("X-API-Key", key);
    }

    let resp = req
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await?;

    let layer = resp
        .headers()
        .get("X-Isartor-Layer")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();
    let deflected = resp
        .headers()
        .get("X-Isartor-Deflected")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == "true")
        .unwrap_or(false);
    let status = resp.status();

    let body: Value = resp
        .json()
        .await
        .unwrap_or(json!({"error": "empty response"}));

    let answer = body
        .get("response")
        .and_then(|r| r.as_str())
        .unwrap_or_else(|| {
            body.get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("message"))
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
                .unwrap_or("No response")
        });

    let meta = format!("[layer={layer}, deflected={deflected}, status={status}]");

    Ok(json!({
        "content": [{
            "type": "text",
            "text": format!("{answer}\n\n{meta}")
        }],
        "isError": false
    }))
}

// ── JSON-RPC helpers ──────────────────────────────────────────────────

fn jsonrpc_ok(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn jsonrpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    })
}

fn send(out: &mut impl Write, msg: &Value) -> io::Result<()> {
    let s = serde_json::to_string(msg).unwrap_or_default();
    writeln!(out, "{s}")?;
    out.flush()
}
