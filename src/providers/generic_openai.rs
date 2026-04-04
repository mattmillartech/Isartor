//! Generic OpenAI-compatible agent that forwards requests to any configured base URL.
//! Used when `ISARTOR__EXTERNAL_LLM_URL` is set and the provider is `openai`.

use anyhow::{Context, anyhow, bail};
use reqwest::Client;
use serde_json::{Value, json};
use std::time::Duration;

use crate::state::AppLlmAgent;

#[derive(Debug, Clone)]
pub struct GenericOpenAIAgent {
    http: Client,
    base_url: String,
    api_key: String,
    model: String,
    #[allow(dead_code)]
    request_timeout: Duration,
}

impl GenericOpenAIAgent {
    pub fn new(
        http: Client,
        base_url: String,
        api_key: String,
        model: String,
        request_timeout: Duration,
    ) -> Self {
        // Normalize base URL: strip trailing /v1/chat/completions or /v1 suffix,
        // we'll add the path ourselves.
        let normalized = base_url
            .trim_end_matches("/v1/chat/completions")
            .trim_end_matches("/v1")
            .trim_end_matches('/')
            .to_string();
        Self {
            http,
            base_url: normalized,
            api_key,
            model,
            request_timeout,
        }
    }

    fn completions_url(&self) -> String {
        format!("{}/v1/chat/completions", self.base_url)
    }
}

#[async_trait::async_trait]
impl AppLlmAgent for GenericOpenAIAgent {
    async fn chat(&self, prompt: &str) -> anyhow::Result<String> {
        let body = json!({
            "model": self.model,
            "messages": [{"role": "user", "content": prompt}],
        });

        let url = self.completions_url();
        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .with_context(|| format!("OpenAI-compat request to {url} failed"))?;

        let status = response.status();
        let payload: Value = response
            .json()
            .await
            .context("failed to parse OpenAI-compat response")?;

        if !status.is_success() {
            bail!("OpenAI-compat returned HTTP {status}: {}", payload);
        }

        payload["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                anyhow!(
                    "OpenAI-compat response did not contain assistant text: {}",
                    payload
                )
            })
    }

    fn provider_name(&self) -> &'static str {
        "openai"
    }
}
