use clap::Parser;

use super::{
    home_path, remove_file, test_isartor_connection, write_file, BaseClientArgs, ConfigChange,
    ConfigChangeType, ConnectResult,
};

#[derive(Parser, Debug, Clone)]
pub struct CopilotArgs {
    #[command(flatten)]
    pub base: BaseClientArgs,

    /// GitHub personal access token (ghp_... or gho_...)
    #[arg(long, env = "GITHUB_TOKEN")]
    pub github_token: Option<String>,

    /// Shell to write exports to: bash | zsh | fish | powershell
    #[arg(long, default_value = "bash")]
    pub shell: String,
}

pub async fn handle_copilot_connect(args: CopilotArgs) -> ConnectResult {
    let gateway = args.base.effective_gateway_url();
    let gateway_key = args.base.effective_gateway_api_key();

    let mut changes = Vec::new();

    if args.base.disconnect {
        return disconnect(&args, &mut changes);
    }

    // NOTE: Some clients use HTTPS_PROXY (CONNECT) which Isartor does not implement.
    // We still write the requested env file scaffold.
    let proxy_url = gateway.clone();

    let env_content = match args.shell.as_str() {
        "fish" => format!(
            "# Isartor — GitHub Copilot CLI integration\n\
             # Source this file: source ~/.isartor/env/copilot.fish\n\
             set -x HTTPS_PROXY \"{}\"\n\
             set -x ISARTOR_COPILOT_ENABLED true\n",
            proxy_url
        ),
        "powershell" => format!(
            "# Isartor — GitHub Copilot CLI integration\n\
             # Dot-source: . ~/.isartor/env/copilot.ps1\n\
             $env:HTTPS_PROXY = \"{}\"\n\
             $env:ISARTOR_COPILOT_ENABLED = \"true\"\n",
            proxy_url
        ),
        _ => format!(
            "# Isartor — GitHub Copilot CLI integration\n\
             # Source this file: source ~/.isartor/env/copilot.sh\n\
             export HTTPS_PROXY=\"{}\"\n\
             export ISARTOR_COPILOT_ENABLED=true\n",
            proxy_url
        ),
    };

    let ext = match args.shell.as_str() {
        "fish" => "fish",
        "powershell" => "ps1",
        _ => "sh",
    };

    let env_path = home_path(&format!(".isartor/env/copilot.{ext}"))
        .unwrap_or_else(|_| std::path::PathBuf::from(format!(".isartor/env/copilot.{ext}")));

    if args.base.show_config || args.base.dry_run {
        println!("{}", env_content);
    }

    if write_file(&env_path, &env_content, args.base.dry_run).is_ok() {
        changes.push(ConfigChange {
            change_type: ConfigChangeType::FileCreated,
            target: env_path.to_string_lossy().to_string(),
            description: "Shell env file with HTTPS_PROXY set".to_string(),
        });
    }

    // Store token in a local providers file for future use by the server.
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

    let test = test_isartor_connection(
        &gateway,
        gateway_key.as_deref(),
        "What is the capital of France?",
    )
    .await;

    let source_cmd = match args.shell.as_str() {
        "fish" => format!("source {}", env_path.display()),
        "powershell" => format!(". {}", env_path.display()),
        _ => format!("source {}", env_path.display()),
    };

    ConnectResult {
        client_name: "GitHub Copilot CLI".to_string(),
        success: test.response_received || args.base.dry_run,
        message: format!(
            "Run this to activate in your current shell:\n  {}\n\nNote: HTTPS proxying may require CONNECT proxy support. If Copilot CLI supports a base URL override, prefer pointing it to {}/v1.",
            source_cmd,
            gateway.trim_end_matches('/')
        ),
        changes_made: changes,
        test_result: Some(test),
    }
}

fn disconnect(args: &CopilotArgs, changes: &mut Vec<ConfigChange>) -> ConnectResult {
    for ext in ["sh", "fish", "ps1"] {
        let path = home_path(&format!(".isartor/env/copilot.{ext}"))
            .unwrap_or_else(|_| std::path::PathBuf::from(format!(".isartor/env/copilot.{ext}")));
        if path.exists() {
            remove_file(&path, args.base.dry_run);
            changes.push(ConfigChange {
                change_type: ConfigChangeType::FileModified,
                target: path.to_string_lossy().to_string(),
                description: "Removed".to_string(),
            });
        }
    }

    ConnectResult {
        client_name: "GitHub Copilot CLI".to_string(),
        success: true,
        message: "Copilot disconnected. Restart your shell to unset HTTPS_PROXY.".to_string(),
        changes_made: changes.clone(),
        test_result: None,
    }
}
