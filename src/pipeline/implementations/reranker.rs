#![allow(dead_code)]
#![allow(clippy::trim_split_whitespace)]
// =============================================================================
// LlamaCppReranker — Layer 2.5 Cross-Encoder-style reranker via llama.cpp.
//
// Scores each (prompt, document) pair by asking the local llama.cpp sidecar
// to rate relevance on a 0–10 scale.  Sorts by descending score, returns top-K.
//
// This is a practical production approach when you don't have a dedicated
// Cross-Encoder ONNX model.  The SLM acts as a semantic judge.
//
// Lightweight Sidecar Strategy:
//   Uses the OpenAI-compatible `/v1/chat/completions` endpoint.
// =============================================================================

use async_trait::async_trait;

use crate::pipeline::traits::Reranker;

/// System prompt that constrains the SLM to output only a relevance score.
const RERANK_SYSTEM_PROMPT: &str = "\
You are a relevance scorer. Given a user QUERY and a DOCUMENT, rate how \
relevant the document is to the query on a scale from 0 to 10.\n\n\
- 0 means completely irrelevant.\n\
- 10 means perfectly relevant and directly answers the query.\n\n\
Reply with ONLY a single number (integer or decimal). No explanation.";

// ── OpenAI-compatible request/response types (private) ──────────────────────

#[derive(serde::Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(serde::Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    temperature: f32,
    max_tokens: u32,
}

#[derive(serde::Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(serde::Deserialize)]
struct ChatChoiceMessage {
    content: String,
}

#[derive(serde::Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

// ── Reranker implementation ─────────────────────────────────────────────────

/// Production reranker that uses the llama.cpp sidecar to score (prompt, document) pairs.
pub struct LlamaCppReranker {
    http_client: reqwest::Client,
    completions_url: String,
    model: String,
}

// Keep old name as alias for ergonomic migration.
pub type OllamaReranker = LlamaCppReranker;

impl LlamaCppReranker {
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

    /// Score a single (query, document) pair via the local SLM.
    async fn score_document(&self, query: &str, document: &str) -> f64 {
        let user_content =
            format!("QUERY: {query}\n\nDOCUMENT: {document}\n\nRelevance score (0-10):");

        let req = ChatCompletionRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: RERANK_SYSTEM_PROMPT.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: user_content,
                },
            ],
            stream: false,
            temperature: 0.0,
            max_tokens: 10,
        };

        let result = self
            .http_client
            .post(&self.completions_url)
            .json(&req)
            .send()
            .await;

        match result {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<ChatCompletionResponse>().await {
                    Ok(completion) => {
                        let text = completion
                            .choices
                            .into_iter()
                            .next()
                            .map(|c| c.message.content)
                            .unwrap_or_default();
                        parse_score(&text)
                    }
                    Err(_) => 0.0,
                }
            }
            _ => 0.0, // Network failure → score 0 (document will be ranked last).
        }
    }
}

/// Parse a relevance score (0–10) from the model's response text.
///
/// Handles formats like "7", "7.5", "Score: 8", etc.
fn parse_score(text: &str) -> f64 {
    let trimmed = text.trim();

    // Try direct parse first.
    if let Ok(val) = trimmed.parse::<f64>() {
        return val.clamp(0.0, 10.0);
    }

    // Fallback: extract the first float-like token.
    for token in trimmed.split_whitespace() {
        let cleaned: String = token
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if let Ok(val) = cleaned.parse::<f64>() {
            return val.clamp(0.0, 10.0);
        }
    }

    0.0
}

#[async_trait]
impl Reranker for LlamaCppReranker {
    async fn rerank(
        &self,
        prompt: &str,
        documents: &[String],
        top_k: usize,
    ) -> anyhow::Result<Vec<(String, f64)>> {
        // Score all documents concurrently using Tokio tasks.
        let mut handles = Vec::with_capacity(documents.len());

        for doc in documents {
            let query = prompt.to_string();
            let document = doc.clone();
            let client = self.http_client.clone();
            let url = self.completions_url.clone();
            let model = self.model.clone();

            handles.push(tokio::spawn(async move {
                let reranker = LlamaCppReranker {
                    http_client: client,
                    completions_url: url,
                    model,
                };
                let score = reranker.score_document(&query, &document).await;
                (document, score)
            }));
        }

        let mut scored: Vec<(String, f64)> = Vec::with_capacity(documents.len());
        for handle in handles {
            match handle.await {
                Ok(result) => scored.push(result),
                Err(e) => {
                    tracing::warn!(error = %e, "Reranker: scoring task panicked");
                }
            }
        }

        // Sort by descending relevance score.
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Normalise scores from 0–10 to 0.0–1.0 for the pipeline.
        let result: Vec<(String, f64)> = scored
            .into_iter()
            .take(top_k)
            .map(|(doc, score)| (doc, score / 10.0))
            .collect();

        tracing::debug!(
            input_docs = documents.len(),
            output_docs = result.len(),
            model = %self.model,
            "LlamaCppReranker: reranking complete"
        );

        Ok(result)
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::traits::Reranker;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn mock_score_response(score: &str) -> serde_json::Value {
        serde_json::json!({
            "choices": [{
                "message": {"content": score},
                "index": 0,
                "finish_reason": "stop"
            }]
        })
    }

    // ── parse_score ──────────────────────────────────────────────

    #[test]
    fn parse_score_integer() {
        assert!((parse_score("7") - 7.0).abs() < 1e-9);
    }

    #[test]
    fn parse_score_decimal() {
        assert!((parse_score("7.5") - 7.5).abs() < 1e-9);
    }

    #[test]
    fn parse_score_with_prefix() {
        assert!((parse_score("Score: 8") - 8.0).abs() < 1e-9);
    }

    #[test]
    fn parse_score_clamped_high() {
        assert!((parse_score("15") - 10.0).abs() < 1e-9);
    }

    #[test]
    fn parse_score_clamped_low() {
        assert!((parse_score("-5") - 0.0).abs() < 1e-9);
    }

    #[test]
    fn parse_score_no_number() {
        assert_eq!(parse_score("no number here"), 0.0);
    }

    #[test]
    fn parse_score_whitespace() {
        assert!((parse_score("  8  ") - 8.0).abs() < 1e-9);
    }

    #[test]
    fn parse_score_empty() {
        assert_eq!(parse_score(""), 0.0);
    }

    // ── Constructor ──────────────────────────────────────────────

    #[test]
    fn constructor_appends_path() {
        let client = reqwest::Client::new();
        let r = LlamaCppReranker::new(client, "http://localhost:8081", "model".into());
        assert_eq!(
            r.completions_url,
            "http://localhost:8081/v1/chat/completions"
        );
    }

    #[test]
    fn model_name_accessor() {
        let client = reqwest::Client::new();
        let r = LlamaCppReranker::new(client, "http://localhost:8081", "phi-3".into());
        assert_eq!(r.model_name(), "phi-3");
    }

    // ── score_document ───────────────────────────────────────────

    #[tokio::test]
    async fn score_document_success() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_score_response("8")))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let reranker = LlamaCppReranker::new(client, &server.uri(), "model".into());
        let score = reranker.score_document("query", "document").await;
        assert!((score - 8.0).abs() < 1e-6);
    }

    #[tokio::test]
    async fn score_document_failure_returns_zero() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let reranker = LlamaCppReranker::new(client, &server.uri(), "model".into());
        let score = reranker.score_document("query", "document").await;
        assert_eq!(score, 0.0);
    }

    // ── rerank ───────────────────────────────────────────────────

    #[tokio::test]
    async fn rerank_returns_top_k_normalised() {
        let server = MockServer::start().await;

        // Return a fixed score for all documents.
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_score_response("5")))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let reranker = LlamaCppReranker::new(client, &server.uri(), "model".into());
        let docs = vec!["doc1".to_string(), "doc2".to_string(), "doc3".to_string()];
        let result = reranker.rerank("query", &docs, 2).await.unwrap();

        assert_eq!(result.len(), 2);
        // Scores should be normalised to 0.0–1.0 (5/10 = 0.5).
        for (_, score) in &result {
            assert!((*score - 0.5).abs() < 1e-6);
        }
    }

    #[tokio::test]
    async fn rerank_empty_docs() {
        let server = MockServer::start().await;
        let client = reqwest::Client::new();
        let reranker = LlamaCppReranker::new(client, &server.uri(), "model".into());
        let docs: Vec<String> = vec![];
        let result = reranker.rerank("query", &docs, 5).await.unwrap();
        assert!(result.is_empty());
    }
}
