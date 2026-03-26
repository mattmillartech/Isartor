use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use clap::Parser;
use serde_json::{Map, Value};

use super::{
    BaseClientArgs, ConfigChange, ConfigChangeType, ConnectResult, remove_file,
    test_isartor_connection, write_file,
};

const CLAUDE_DESKTOP_CONFIG_ENV: &str = "CLAUDE_DESKTOP_CONFIG_PATH";
const SERVER_NAME: &str = "isartor";

#[derive(Parser, Debug, Clone)]
pub struct ClaudeDesktopArgs {
    #[command(flatten)]
    pub base: BaseClientArgs,
}

pub async fn handle_claude_desktop_connect(args: ClaudeDesktopArgs) -> ConnectResult {
    let gateway = args.base.effective_gateway_url();
    let gateway_key = args.base.effective_gateway_api_key();
    let mut changes = Vec::new();

    let config_path = match detect_claude_desktop_config_path() {
        Ok(path) => path,
        Err(err) => {
            return ConnectResult {
                client_name: "Claude Desktop".to_string(),
                success: false,
                message: format!("Could not locate Claude Desktop config: {err}"),
                changes_made: changes,
                test_result: None,
            };
        }
    };

    if args.base.disconnect {
        return disconnect_claude_desktop(&args, &config_path, &mut changes);
    }

    let backup_path = backup_path_for(&config_path);
    if let Err(err) = backup_file(&config_path, &backup_path, args.base.dry_run, &mut changes) {
        return failure_result(
            format!("Failed to back up Claude Desktop config: {err}"),
            changes,
        );
    }

    let existing = match read_config_json(&config_path) {
        Ok(existing) => existing,
        Err(err) => {
            return failure_result(
                format!(
                    "Failed to read Claude Desktop config from {}: {err}",
                    config_path.display()
                ),
                changes,
            );
        }
    };

    let isartor_bin = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("isartor"));
    let updated = build_claude_desktop_config(
        existing,
        build_isartor_mcp_server(
            &isartor_bin,
            &gateway,
            gateway_key.as_deref().filter(|key| !key.trim().is_empty()),
        ),
    );
    let rendered = match serde_json::to_string_pretty(&updated) {
        Ok(rendered) => format!("{rendered}\n"),
        Err(err) => {
            return failure_result(
                format!("Failed to render Claude Desktop config: {err}"),
                changes,
            );
        }
    };

    if args.base.show_config || args.base.dry_run {
        println!("--- {} ---\n{rendered}", config_path.display());
    }

    let existed = config_path.exists();
    if let Err(err) = write_file(&config_path, &rendered, args.base.dry_run) {
        return failure_result(
            format!(
                "Failed to write Claude Desktop config to {}: {err}",
                config_path.display()
            ),
            changes,
        );
    }

    changes.push(ConfigChange {
        change_type: if existed {
            ConfigChangeType::FileModified
        } else {
            ConfigChangeType::FileCreated
        },
        target: config_path.to_string_lossy().to_string(),
        description: "Registered Isartor as a local MCP server for Claude Desktop".to_string(),
    });

    let test = test_isartor_connection(
        &gateway,
        gateway_key.as_deref(),
        "Claude Desktop MCP preflight check",
    )
    .await;
    let success = test.response_received || test.layer_resolved == "timeout" || args.base.dry_run;

    ConnectResult {
        client_name: "Claude Desktop".to_string(),
        success,
        message: format!(
            "Configured Claude Desktop to launch Isartor as a local MCP server.\n\n\
             Claude Desktop config:\n  {}\n\
             Backup:\n  {}\n\n\
             What to do next:\n\
             1. Start Isartor: `isartor up --detach`\n\
             2. Restart Claude Desktop\n\
             3. In Claude Desktop, open the tools/connectors UI to confirm the `isartor` MCP server is available\n\n\
             Method: local MCP stdio server (`isartor mcp`)\n\
             Advanced: Isartor also exposes MCP over HTTP/SSE at `{}/mcp/` for clients that support remote MCP registration.",
            config_path.display(),
            backup_path.display(),
            gateway.trim_end_matches('/'),
        ),
        changes_made: changes,
        test_result: Some(test),
    }
}

fn disconnect_claude_desktop(
    args: &ClaudeDesktopArgs,
    config_path: &Path,
    changes: &mut Vec<ConfigChange>,
) -> ConnectResult {
    let backup_path = backup_path_for(config_path);
    let outcome = if backup_path.exists() {
        restore_file(
            config_path,
            &backup_path,
            args.base.dry_run,
            changes,
            "Restored Claude Desktop config from backup",
        )
        .map(|_| "Restored the original Claude Desktop config from backup. Restart Claude Desktop to apply changes.".to_string())
    } else {
        remove_isartor_server(config_path, args.base.dry_run, changes).map(|removed| {
            if removed {
                "Removed Isartor from Claude Desktop's MCP server list. Restart Claude Desktop to apply changes.".to_string()
            } else {
                "No managed Claude Desktop Isartor MCP entry was present. Nothing to disconnect.".to_string()
            }
        })
    };

    match outcome {
        Ok(message) => ConnectResult {
            client_name: "Claude Desktop".to_string(),
            success: true,
            message,
            changes_made: changes.clone(),
            test_result: None,
        },
        Err(err) => failure_result(
            format!("Failed to disconnect Claude Desktop from Isartor: {err}"),
            changes.clone(),
        ),
    }
}

fn failure_result(message: String, changes: Vec<ConfigChange>) -> ConnectResult {
    ConnectResult {
        client_name: "Claude Desktop".to_string(),
        success: false,
        message,
        changes_made: changes,
        test_result: None,
    }
}

fn detect_claude_desktop_config_path() -> anyhow::Result<PathBuf> {
    if let Some(override_path) = std::env::var_os(CLAUDE_DESKTOP_CONFIG_ENV)
        && !override_path.is_empty()
    {
        return Ok(PathBuf::from(override_path));
    }

    let home_dir = dirs::home_dir();
    let xdg_config_home = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    let appdata = std::env::var_os("APPDATA").map(PathBuf::from);

    detect_claude_desktop_config_path_for(
        std::env::consts::OS,
        home_dir.as_deref(),
        xdg_config_home.as_deref(),
        appdata.as_deref(),
    )
}

fn detect_claude_desktop_config_path_for(
    os: &str,
    home_dir: Option<&Path>,
    xdg_config_home: Option<&Path>,
    appdata: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    match os {
        "macos" => home_dir
            .map(|home| home.join("Library/Application Support/Claude/claude_desktop_config.json"))
            .ok_or_else(|| anyhow!("HOME is not set")),
        "linux" => Ok(match xdg_config_home {
            Some(xdg) => xdg.join("Claude/claude_desktop_config.json"),
            None => home_dir
                .map(|home| home.join(".config/Claude/claude_desktop_config.json"))
                .ok_or_else(|| anyhow!("HOME is not set and XDG_CONFIG_HOME is not set"))?,
        }),
        "windows" => {
            if let Some(appdata) = appdata {
                Ok(appdata.join("Claude/claude_desktop_config.json"))
            } else if let Some(home) = home_dir {
                Ok(home.join("AppData/Roaming/Claude/claude_desktop_config.json"))
            } else {
                Err(anyhow!("APPDATA is not set and HOME is not set"))
            }
        }
        other => Err(anyhow!("unsupported operating system: {other}")),
    }
}

fn read_config_json(path: &Path) -> anyhow::Result<Value> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    if content.trim().is_empty() {
        return Ok(serde_json::json!({}));
    }

    let value: Value =
        json5::from_str(&content).with_context(|| format!("failed to parse {}", path.display()))?;
    if !value.is_object() {
        return Err(anyhow!("expected a JSON object at {}", path.display()));
    }
    Ok(value)
}

fn build_isartor_mcp_server(
    isartor_bin: &Path,
    gateway_url: &str,
    gateway_api_key: Option<&str>,
) -> Value {
    let mut env = Map::new();
    env.insert(
        "ISARTOR_GATEWAY_URL".to_string(),
        Value::String(gateway_url.to_string()),
    );
    if let Some(api_key) = gateway_api_key {
        env.insert(
            "ISARTOR__GATEWAY_API_KEY".to_string(),
            Value::String(api_key.to_string()),
        );
    }

    serde_json::json!({
        "command": isartor_bin.to_string_lossy(),
        "args": ["mcp"],
        "env": env,
    })
}

fn build_claude_desktop_config(existing: Value, isartor_entry: Value) -> Value {
    let mut root = as_object(existing);
    let mut servers = root
        .remove("mcpServers")
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    servers.insert(SERVER_NAME.to_string(), isartor_entry);
    root.insert("mcpServers".to_string(), Value::Object(servers));
    Value::Object(root)
}

fn remove_isartor_server(
    config_path: &Path,
    dry_run: bool,
    changes: &mut Vec<ConfigChange>,
) -> anyhow::Result<bool> {
    if !config_path.exists() {
        return Ok(false);
    }

    let mut root = read_config_json(config_path)?;
    let removed = root
        .get_mut("mcpServers")
        .and_then(|value| value.as_object_mut())
        .and_then(|servers| servers.remove(SERVER_NAME))
        .is_some();
    if !removed {
        return Ok(false);
    }

    let rendered = format!("{}\n", serde_json::to_string_pretty(&root)?);
    write_file(config_path, &rendered, dry_run)?;
    changes.push(ConfigChange {
        change_type: ConfigChangeType::FileModified,
        target: config_path.to_string_lossy().to_string(),
        description: "Removed Isartor MCP registration from Claude Desktop".to_string(),
    });
    Ok(true)
}

fn backup_path_for(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| format!("{}.isartor-backup", name.to_string_lossy()))
        .unwrap_or_else(|| "claude_desktop_config.json.isartor-backup".to_string());
    path.with_file_name(file_name)
}

fn backup_file(
    original_path: &Path,
    backup_path: &Path,
    dry_run: bool,
    changes: &mut Vec<ConfigChange>,
) -> anyhow::Result<()> {
    if !original_path.exists() || backup_path.exists() {
        return Ok(());
    }
    let content = std::fs::read_to_string(original_path)
        .with_context(|| format!("failed to read {}", original_path.display()))?;
    write_file(backup_path, &content, dry_run)?;
    changes.push(ConfigChange {
        change_type: ConfigChangeType::FileBackedUp,
        target: backup_path.to_string_lossy().to_string(),
        description: "Original Claude Desktop config backed up".to_string(),
    });
    Ok(())
}

fn restore_file(
    original_path: &Path,
    backup_path: &Path,
    dry_run: bool,
    changes: &mut Vec<ConfigChange>,
    description: &str,
) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(backup_path)
        .with_context(|| format!("failed to read {}", backup_path.display()))?;
    if content.is_empty() {
        remove_file(original_path, dry_run);
    } else {
        write_file(original_path, &content, dry_run)?;
    }
    remove_file(backup_path, dry_run);
    changes.push(ConfigChange {
        change_type: ConfigChangeType::FileModified,
        target: original_path.to_string_lossy().to_string(),
        description: description.to_string(),
    });
    Ok(())
}

fn as_object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn detect_config_path_for_macos_uses_application_support() {
        let path = detect_claude_desktop_config_path_for(
            "macos",
            Some(Path::new("/Users/test")),
            None,
            None,
        )
        .unwrap();

        assert_eq!(
            path,
            PathBuf::from(
                "/Users/test/Library/Application Support/Claude/claude_desktop_config.json"
            )
        );
    }

    #[test]
    fn detect_config_path_for_linux_prefers_xdg() {
        let path = detect_claude_desktop_config_path_for(
            "linux",
            Some(Path::new("/home/test")),
            Some(Path::new("/tmp/xdg")),
            None,
        )
        .unwrap();

        assert_eq!(
            path,
            PathBuf::from("/tmp/xdg/Claude/claude_desktop_config.json")
        );
    }

    #[test]
    fn detect_config_path_for_windows_prefers_appdata() {
        let path = detect_claude_desktop_config_path_for(
            "windows",
            Some(Path::new("C:/Users/test")),
            None,
            Some(Path::new("C:/Users/test/AppData/Roaming")),
        )
        .unwrap();

        assert_eq!(
            path,
            PathBuf::from("C:/Users/test/AppData/Roaming/Claude/claude_desktop_config.json")
        );
    }

    #[test]
    fn build_config_preserves_existing_servers_and_fields() {
        let existing = serde_json::json!({
            "theme": "dark",
            "mcpServers": {
                "filesystem": {
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-filesystem"]
                }
            }
        });

        let updated = build_claude_desktop_config(
            existing,
            serde_json::json!({"command": "isartor", "args": ["mcp"], "env": {}}),
        );

        assert_eq!(updated["theme"], "dark");
        assert!(updated["mcpServers"]["filesystem"].is_object());
        assert_eq!(updated["mcpServers"]["isartor"]["command"], "isartor");
    }

    #[test]
    fn build_isartor_mcp_server_omits_empty_api_key() {
        let entry = build_isartor_mcp_server(
            Path::new("/usr/local/bin/isartor"),
            "http://localhost:8080",
            None,
        );

        assert_eq!(entry["command"], "/usr/local/bin/isartor");
        assert_eq!(entry["args"], serde_json::json!(["mcp"]));
        assert_eq!(entry["env"]["ISARTOR_GATEWAY_URL"], "http://localhost:8080");
        assert!(entry["env"].get("ISARTOR__GATEWAY_API_KEY").is_none());
    }

    #[test]
    fn remove_isartor_server_only_removes_managed_entry() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("claude_desktop_config.json");
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&serde_json::json!({
                "mcpServers": {
                    "filesystem": {"command": "npx"},
                    "isartor": {"command": "isartor", "args": ["mcp"]}
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let mut changes = Vec::new();
        let removed = remove_isartor_server(&path, false, &mut changes).unwrap();
        let updated: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();

        assert!(removed);
        assert!(updated["mcpServers"]["filesystem"].is_object());
        assert!(updated["mcpServers"].get("isartor").is_none());
        assert_eq!(changes.len(), 1);
    }

    #[test]
    fn backup_and_restore_round_trip() {
        let dir = tempdir().unwrap();
        let original = dir.path().join("claude_desktop_config.json");
        let backup = backup_path_for(&original);
        std::fs::write(&original, "{\"mcpServers\":{\"other\":{}}}\n").unwrap();

        let mut changes = Vec::new();
        backup_file(&original, &backup, false, &mut changes).unwrap();
        std::fs::write(&original, "{\"mcpServers\":{\"isartor\":{}}}\n").unwrap();
        restore_file(&original, &backup, false, &mut changes, "restore").unwrap();

        assert_eq!(
            std::fs::read_to_string(&original).unwrap(),
            "{\"mcpServers\":{\"other\":{}}}\n"
        );
        assert!(!backup.exists());
    }
}
