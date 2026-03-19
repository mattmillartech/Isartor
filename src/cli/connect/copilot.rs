use clap::Parser;

use super::{
    BaseClientArgs, ConfigChange, ConfigChangeType, ConnectResult, home_path, remove_file,
    test_isartor_connection, write_file,
};

#[derive(Parser, Debug, Clone)]
pub struct CopilotArgs {
    #[command(flatten)]
    pub base: BaseClientArgs,

    /// GitHub personal access token (ghp_... or gho_...)
    #[arg(long, env = "GITHUB_TOKEN")]
    pub github_token: Option<String>,
}

pub async fn handle_copilot_connect(args: CopilotArgs) -> ConnectResult {
    let gateway = args.base.effective_gateway_url();
    let gateway_key = args.base.effective_gateway_api_key();

    let mut changes = Vec::new();

    if args.base.disconnect {
        return disconnect(&args, &mut changes);
    }

    // Clean up legacy files from previous integration approaches.
    if !args.base.dry_run {
        cleanup_legacy_files(&mut changes);
    }

    // Step 1: Find the isartor binary path for MCP config.
    let isartor_bin =
        std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("isartor"));

    // Step 2: Build MCP server entry for ~/.copilot/mcp-config.json
    let mut mcp_args = vec!["mcp".to_string()];
    if gateway != super::DEFAULT_GATEWAY_URL {
        mcp_args.push("--gateway-url".to_string());
        mcp_args.push(gateway.clone());
    }
    if let Some(ref key) = gateway_key
        && !key.is_empty()
    {
        mcp_args.push("--gateway-api-key".to_string());
        mcp_args.push(key.clone());
    }

    let isartor_entry = serde_json::json!({
        "type": "stdio",
        "command": isartor_bin.to_string_lossy(),
        "args": mcp_args,
    });

    // Step 3: Read existing mcp-config.json, merge, write back.
    let mcp_config_path = home_path(".copilot/mcp-config.json")
        .unwrap_or_else(|_| std::path::PathBuf::from(".copilot/mcp-config.json"));

    let mut mcp_config: serde_json::Value = if mcp_config_path.exists() {
        let content = std::fs::read_to_string(&mcp_config_path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    // Support both { "mcpServers": { ... } } and flat { "server": { ... } } formats.
    // Copilot CLI accepts both; prefer mcpServers wrapper.
    if mcp_config.get("mcpServers").is_none() {
        mcp_config["mcpServers"] = serde_json::json!({});
    }
    mcp_config["mcpServers"]["isartor"] = isartor_entry;

    let mcp_json = serde_json::to_string_pretty(&mcp_config).unwrap_or_default();

    if args.base.show_config || args.base.dry_run {
        println!("{}", mcp_json);
    }

    let mcp_written = if !args.base.dry_run {
        write_file(&mcp_config_path, &mcp_json, false).is_ok()
    } else {
        true
    };

    if mcp_written {
        changes.push(ConfigChange {
            change_type: if mcp_config_path.exists() {
                ConfigChangeType::FileModified
            } else {
                ConfigChangeType::FileCreated
            },
            target: mcp_config_path.to_string_lossy().to_string(),
            description: "Registered isartor MCP server".to_string(),
        });
    }

    // Step 4: Store GitHub token if provided.
    if let Some(token) = &args.github_token {
        let token_path = home_path(".isartor/providers/copilot.json")
            .unwrap_or_else(|_| std::path::PathBuf::from(".isartor/providers/copilot.json"));
        let cfg = serde_json::json!({
            "provider": "copilot",
            "github_token": token,
        });
        let content = serde_json::to_string_pretty(&cfg).unwrap_or_default();

        if args.base.show_config || args.base.dry_run {
            println!("\n{}", content);
        }

        if write_file(&token_path, &content, args.base.dry_run).is_ok() {
            changes.push(ConfigChange {
                change_type: ConfigChangeType::FileCreated,
                target: token_path.to_string_lossy().to_string(),
                description: "Copilot credentials (local)".to_string(),
            });
        }
    }

    // Step 5: Ensure Isartor gateway URL is in Copilot's allowed URLs.
    add_allowed_url(&gateway, &mut changes, args.base.dry_run);

    // Step 6: Test the gateway connection.
    let test = test_isartor_connection(
        &gateway,
        gateway_key.as_deref(),
        "What is the capital of France?",
    )
    .await;

    let success = test.response_received || test.layer_resolved == "timeout" || args.base.dry_run;

    ConnectResult {
        client_name: "GitHub Copilot CLI".to_string(),
        success,
        message: format!(
            "MCP server registered in:\n  {}\n\n\
             Copilot CLI will now have an `isartor_chat` tool available.\n\
             Prompts sent through this tool route through the deflection stack\n\
             (L1a/L1b cache → L2 SLM → L3 cloud).\n\n\
             Start Copilot CLI normally — no env vars or hooks needed:\n  copilot",
            mcp_config_path.display(),
        ),
        changes_made: changes,
        test_result: Some(test),
    }
}

/// Add the gateway URL to Copilot's allowed_urls in ~/.copilot/config.json.
fn add_allowed_url(gateway_url: &str, changes: &mut Vec<ConfigChange>, dry_run: bool) {
    let config_path = match home_path(".copilot/config.json") {
        Ok(p) => p,
        Err(_) => return,
    };
    if !config_path.exists() {
        return;
    }

    let content = match std::fs::read_to_string(&config_path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let mut config: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return,
    };

    let urls = config
        .get_mut("allowed_urls")
        .and_then(|v| v.as_array_mut());

    if let Some(urls) = urls {
        let gw = serde_json::Value::String(gateway_url.to_string());
        if !urls.contains(&gw) {
            urls.push(gw);
            if !dry_run && let Ok(json) = serde_json::to_string_pretty(&config) {
                let _ = std::fs::write(&config_path, json);
                changes.push(ConfigChange {
                    change_type: ConfigChangeType::FileModified,
                    target: config_path.to_string_lossy().to_string(),
                    description: format!("Added {gateway_url} to allowed_urls"),
                });
            }
        }
    }
}

fn cleanup_legacy_files(changes: &mut Vec<ConfigChange>) {
    // Legacy proxy-era env files (v0.1.33 and earlier).
    for ext in ["sh", "fish", "ps1"] {
        let path = home_path(&format!(".isartor/env/copilot.{ext}")).unwrap_or_default();
        if path.exists() {
            remove_file(&path, false);
            changes.push(ConfigChange {
                change_type: ConfigChangeType::FileModified,
                target: path.to_string_lossy().to_string(),
                description: "Removed legacy proxy env file".to_string(),
            });
        }
    }

    // Legacy hook files (v0.1.34 hook approach).
    for filename in [
        ".isartor/hooks/copilot_pretooluse.sh",
        ".isartor/copilot-hook-setup.txt",
    ] {
        let path = home_path(filename).unwrap_or_default();
        if path.exists() {
            remove_file(&path, false);
            changes.push(ConfigChange {
                change_type: ConfigChangeType::FileModified,
                target: path.to_string_lossy().to_string(),
                description: "Removed legacy hook file".to_string(),
            });
        }
    }
}

fn disconnect(args: &CopilotArgs, changes: &mut Vec<ConfigChange>) -> ConnectResult {
    // Remove isartor entry from mcp-config.json.
    let mcp_config_path = home_path(".copilot/mcp-config.json")
        .unwrap_or_else(|_| std::path::PathBuf::from(".copilot/mcp-config.json"));

    if mcp_config_path.exists()
        && let Ok(content) = std::fs::read_to_string(&mcp_config_path)
        && let Ok(mut config) = serde_json::from_str::<serde_json::Value>(&content)
    {
        let removed = config
            .get_mut("mcpServers")
            .and_then(|s| s.as_object_mut())
            .map(|s| s.remove("isartor").is_some())
            .unwrap_or(false);

        if removed && !args.base.dry_run {
            if let Ok(json) = serde_json::to_string_pretty(&config) {
                let _ = std::fs::write(&mcp_config_path, json);
            }
            changes.push(ConfigChange {
                change_type: ConfigChangeType::FileModified,
                target: mcp_config_path.to_string_lossy().to_string(),
                description: "Removed isartor MCP server".to_string(),
            });
        }
    }

    // Also clean up any legacy files.
    if !args.base.dry_run {
        cleanup_legacy_files(changes);
    }

    ConnectResult {
        client_name: "GitHub Copilot CLI".to_string(),
        success: true,
        message: "Copilot CLI disconnected from Isartor.\n\
                  The isartor MCP server has been removed from ~/.copilot/mcp-config.json."
            .to_string(),
        changes_made: changes.clone(),
        test_result: None,
    }
}
