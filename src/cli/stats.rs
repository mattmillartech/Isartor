use clap::Parser;

use crate::config::AppConfig;
use crate::models::{PromptStatsResponse, PromptVisibilityEntry};

#[derive(Parser, Debug, Clone)]
pub struct StatsArgs {
    /// Isartor gateway URL (default: http://localhost:8080)
    #[arg(long, default_value = "http://localhost:8080")]
    pub gateway_url: String,

    /// Gateway API key (optional). If omitted, stats will try the locally loaded config.
    #[arg(long, env = "ISARTOR__GATEWAY_API_KEY")]
    pub gateway_api_key: Option<String>,

    /// Number of recent prompts to show.
    #[arg(long, default_value_t = 10)]
    pub recent_limit: usize,
}

pub async fn handle_stats(args: StatsArgs) -> anyhow::Result<()> {
    let gateway = args.gateway_url.trim_end_matches('/').to_string();
    let Some(health) = fetch_health(&gateway).await else {
        anyhow::bail!("Isartor is not reachable at {}", gateway);
    };
    let Some(stats) = fetch_prompt_stats(
        &gateway,
        effective_gateway_api_key(args.gateway_api_key.as_deref()),
        args.recent_limit,
    )
    .await
    else {
        anyhow::bail!("Unable to read prompt stats. Provide --gateway-api-key if needed.");
    };

    println!("\nIsartor Prompt Stats");
    println!("  URL:        {}", gateway);
    println!("  Version:    {}", health.version);
    println!("  Total:      {}", stats.total_prompts);
    println!("  Deflected:  {}", stats.total_deflected_prompts);
    println!(
        "  Cloud:      {}",
        stats.by_layer.get("l3").copied().unwrap_or(0)
    );

    println!("\nBy Layer");
    let known_layers = ["l0", "l1a", "l1b", "l2", "l3"];
    for layer in known_layers {
        println!(
            "  {:<3} {}",
            layer.to_uppercase(),
            stats.by_layer.get(layer).copied().unwrap_or(0)
        );
    }
    for (layer, count) in &stats.by_layer {
        if known_layers.contains(&layer.as_str()) {
            continue;
        }
        println!("  {:<3} {}", layer.to_uppercase(), count);
    }

    println!("\nBy Surface");
    for (surface, count) in &stats.by_surface {
        println!("  {:<10} {}", surface, count);
    }

    println!("\nBy Client");
    for (client, count) in &stats.by_client {
        println!("  {:<10} {}", client, count);
    }

    println!("\nRecent Prompts");
    if stats.recent.is_empty() {
        println!("  No prompt traffic recorded yet.");
    } else {
        for entry in stats.recent {
            print_recent_entry(&entry);
        }
    }
    println!();

    Ok(())
}

#[derive(Debug, serde::Deserialize)]
struct Health {
    version: String,
}

async fn fetch_health(gateway: &str) -> Option<Health> {
    let client = reqwest::Client::new();
    client
        .get(format!("{}/health", gateway))
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
        .ok()?
        .json::<Health>()
        .await
        .ok()
}

async fn fetch_prompt_stats(
    gateway: &str,
    gateway_api_key: Option<String>,
    limit: usize,
) -> Option<PromptStatsResponse> {
    let client = reqwest::Client::new();
    let mut req = client
        .get(format!("{}/debug/stats/prompts?limit={}", gateway, limit))
        .timeout(std::time::Duration::from_secs(2));
    if let Some(key) = gateway_api_key {
        req = req.header("X-API-Key", key);
    }
    let resp = req.send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json::<PromptStatsResponse>().await.ok()
}

fn effective_gateway_api_key(cli_value: Option<&str>) -> Option<String> {
    if let Some(value) = cli_value {
        return Some(value.to_string());
    }
    AppConfig::load().ok().map(|cfg| cfg.gateway_api_key)
}

fn print_recent_entry(entry: &PromptVisibilityEntry) {
    println!(
        "  {} {} {} {} via {} ({} ms, HTTP {})",
        entry.timestamp,
        entry.traffic_surface,
        entry.client,
        entry.final_layer.to_uppercase(),
        entry.route,
        entry.latency_ms,
        entry.status_code
    );
}
