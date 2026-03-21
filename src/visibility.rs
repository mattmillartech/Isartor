use std::collections::{BTreeMap, VecDeque};

use axum::{Json, extract::Query, response::IntoResponse};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};

use crate::core::prompt::extract_prompt;
use crate::models::{PromptStatsResponse, PromptVisibilityEntry};

const RECENT_PROMPT_ENTRIES_CAPACITY: usize = 200;

static PROMPT_STATS: std::sync::OnceLock<Mutex<PromptVisibilityState>> = std::sync::OnceLock::new();

#[derive(Debug, Default)]
struct PromptVisibilityState {
    total_prompts: u64,
    total_deflected_prompts: u64,
    by_layer: BTreeMap<String, u64>,
    by_surface: BTreeMap<String, u64>,
    by_client: BTreeMap<String, u64>,
    by_tool: BTreeMap<String, u64>,
    recent: VecDeque<PromptVisibilityEntry>,
}

impl PromptVisibilityState {
    fn record(&mut self, entry: PromptVisibilityEntry) {
        self.total_prompts += 1;
        if entry.deflected {
            self.total_deflected_prompts += 1;
        }
        *self.by_layer.entry(entry.final_layer.clone()).or_insert(0) += 1;
        *self
            .by_surface
            .entry(entry.traffic_surface.clone())
            .or_insert(0) += 1;
        *self.by_client.entry(entry.client.clone()).or_insert(0) += 1;
        if !entry.tool.is_empty() {
            *self.by_tool.entry(entry.tool.clone()).or_insert(0) += 1;
        }

        self.recent.push_front(entry);
        while self.recent.len() > RECENT_PROMPT_ENTRIES_CAPACITY {
            self.recent.pop_back();
        }
    }

    fn snapshot(&self, limit: usize) -> PromptStatsResponse {
        PromptStatsResponse {
            total_prompts: self.total_prompts,
            total_deflected_prompts: self.total_deflected_prompts,
            by_layer: self.by_layer.clone(),
            by_surface: self.by_surface.clone(),
            by_client: self.by_client.clone(),
            by_tool: self.by_tool.clone(),
            recent: self
                .recent
                .iter()
                .take(limit.min(RECENT_PROMPT_ENTRIES_CAPACITY))
                .cloned()
                .collect(),
        }
    }
}

fn prompt_stats_store() -> &'static Mutex<PromptVisibilityState> {
    PROMPT_STATS.get_or_init(|| Mutex::new(PromptVisibilityState::default()))
}

pub fn record_prompt(entry: PromptVisibilityEntry) {
    prompt_stats_store().lock().record(entry);
}

pub fn prompt_stats_snapshot(limit: usize) -> PromptStatsResponse {
    prompt_stats_store().lock().snapshot(limit)
}

#[cfg(test)]
pub fn clear_prompt_stats() {
    *prompt_stats_store().lock() = PromptVisibilityState::default();
}

pub fn prompt_total_requests() -> u64 {
    prompt_stats_store().lock().total_prompts
}

pub fn prompt_total_deflected_requests() -> u64 {
    prompt_stats_store().lock().total_deflected_prompts
}

pub fn prompt_hash_from_body(body: &[u8]) -> Option<String> {
    let prompt = extract_prompt(body);
    if prompt.is_empty() {
        return None;
    }
    Some(hex::encode(Sha256::digest(prompt.as_bytes())))
}

#[derive(Debug, serde::Deserialize)]
pub struct PromptStatsQuery {
    pub limit: Option<usize>,
}

pub async fn prompt_stats_handler(Query(query): Query<PromptStatsQuery>) -> impl IntoResponse {
    Json(prompt_stats_snapshot(query.limit.unwrap_or(20)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_state_tracks_counts_and_recent_entries() {
        let mut state = PromptVisibilityState::default();
        state.record(PromptVisibilityEntry {
            timestamp: "2026-01-01T00:00:00Z".into(),
            traffic_surface: "gateway".into(),
            client: "direct".into(),
            endpoint_family: "native".into(),
            route: "/api/chat".into(),
            prompt_hash: Some("abc".into()),
            final_layer: "l2".into(),
            resolved_by: None,
            deflected: true,
            latency_ms: 12,
            status_code: 200,
            tool: "curl".into(),
        });
        state.record(PromptVisibilityEntry {
            timestamp: "2026-01-01T00:00:01Z".into(),
            traffic_surface: "proxy".into(),
            client: "copilot".into(),
            endpoint_family: "openai".into(),
            route: "copilot-proxy.githubusercontent.com /v1/chat/completions".into(),
            prompt_hash: Some("def".into()),
            final_layer: "l3".into(),
            resolved_by: Some("copilot_upstream".into()),
            deflected: false,
            latency_ms: 20,
            status_code: 200,
            tool: "copilot".into(),
        });

        let snapshot = state.snapshot(10);
        assert_eq!(snapshot.total_prompts, 2);
        assert_eq!(snapshot.total_deflected_prompts, 1);
        assert_eq!(snapshot.by_layer.get("l2"), Some(&1));
        assert_eq!(snapshot.by_layer.get("l3"), Some(&1));
        assert_eq!(snapshot.by_surface.get("gateway"), Some(&1));
        assert_eq!(snapshot.by_surface.get("proxy"), Some(&1));
        assert_eq!(snapshot.by_client.get("direct"), Some(&1));
        assert_eq!(snapshot.by_client.get("copilot"), Some(&1));
        assert_eq!(snapshot.by_tool.get("curl"), Some(&1));
        assert_eq!(snapshot.by_tool.get("copilot"), Some(&1));
        assert_eq!(snapshot.recent.len(), 2);
        assert_eq!(snapshot.recent[0].client, "copilot");
    }

    #[test]
    fn prompt_hash_is_stable_for_supported_payloads() {
        let hash = prompt_hash_from_body(br#"{"prompt":"hello"}"#).unwrap();
        assert_eq!(hash.len(), 64);
    }
}
