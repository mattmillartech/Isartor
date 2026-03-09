#![allow(dead_code)]
#![allow(clippy::trim_split_whitespace)]
// =============================================================================
// LlamaCppLocalExecutor — Layer 2 executor for simple tasks.
//
// When the intent classifier determines a task is "SIMPLE", this executor
// calls the local llama.cpp sidecar to generate a direct answer, bypassing
// the expensive external LLM entirely.
//
// Lightweight Sidecar Strategy:
//   Uses the OpenAI-compatible `/v1/chat/completions` endpoint.
// =============================================================================

use async_trait::async_trait;

use crate::pipeline::traits::LocalExecutor;

/// OpenAI Chat Completions message.
#[derive(serde::Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

/// OpenAI Chat Completions request body.
#[derive(serde::Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
}

/// A single choice in the chat completions response.
#[derive(serde::Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(serde::Deserialize)]
struct ChatChoiceMessage {
    content: String,
}

/// OpenAI Chat Completions response body.
#[derive(serde::Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

/// Production local executor that generates answers via the llama.cpp sidecar.
pub struct LlamaCppLocalExecutor {
    http_client: reqwest::Client,
    completions_url: String,
    model: String,
}

// Keep old name as alias for ergonomic migration.
pub type OllamaLocalExecutor = LlamaCppLocalExecutor;

impl LlamaCppLocalExecutor {
    pub fn new(http_client: reqwest::Client, sidecar_base_url: &str, model: String) -> Self {
        let base = sidecar_base_url.trim_end_matches('/');
        let completions_url = if base.ends_with("/v1/chat/completions") {
            base.to_string()
        } else {
            format!("{base}/v1/chat/completions")
        };

        Self {
            http_client,
            completions_url,
            model,
        }
    }
}

#[async_trait]
impl LocalExecutor for LlamaCppLocalExecutor {
    async fn execute_simple(&self, prompt: &str) -> anyhow::Result<String> {
        let req = ChatCompletionRequest {
            model: self.model.clone(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            stream: false,
        };

        let resp = self
            .http_client
            .post(&self.completions_url)
            .json(&req)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("LlamaCpp generate request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("LlamaCpp generate returned {status}: {body}");
        }

        let completion: ChatCompletionResponse = resp
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse LlamaCpp generate response: {e}"))?;

        let content = completion
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap_or_default();

        tracing::debug!(
            response_len = content.len(),
            model = %self.model,
            "LlamaCppLocalExecutor: simple task completed"
        );

        Ok(content)
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::traits::LocalExecutor;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn mock_response(content: &str) -> serde_json::Value {
        serde_json::json!({
            "choices": [{
                "message": {"content": content},
                "index": 0,
                "finish_reason": "stop"
            }]
        })
    }

    #[test]
    fn constructor_appends_path() {
        let client = reqwest::Client::new();
        let e = LlamaCppLocalExecutor::new(client, "http://localhost:8081", "model".into());
        assert_eq!(
            e.completions_url,
            "http://localhost:8081/v1/chat/completions"
        );
    }

    #[test]
    fn model_name_accessor() {
        let client = reqwest::Client::new();
        let e = LlamaCppLocalExecutor::new(client, "http://localhost:8081", "phi-3".into());
        assert_eq!(e.model_name(), "phi-3");
    }

    #[tokio::test]
    async fn execute_simple_success() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(mock_response("The answer is 4.")),
            )
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let executor = LlamaCppLocalExecutor::new(client, &server.uri(), "test-model".into());
        let result = executor.execute_simple("What is 2+2?").await.unwrap();
        assert_eq!(result, "The answer is 4.");
    }

    #[tokio::test]
    async fn execute_simple_server_error() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(503).set_body_string("overloaded"))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let executor = LlamaCppLocalExecutor::new(client, &server.uri(), "test-model".into());
        let result = executor.execute_simple("hello").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("503"));
    }

    #[tokio::test]
    async fn execute_simple_malformed_json() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let executor = LlamaCppLocalExecutor::new(client, &server.uri(), "test-model".into());
        let result = executor.execute_simple("hello").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn execute_simple_empty_choices() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"choices": []})),
            )
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let executor = LlamaCppLocalExecutor::new(client, &server.uri(), "test-model".into());
        let result = executor.execute_simple("hello").await.unwrap();
        assert_eq!(result, ""); // No choices → empty string.
    }
}
