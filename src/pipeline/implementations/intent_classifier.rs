// =============================================================================
// LlamaCppIntentClassifier — Layer 2 Intent Classifier backed by a
// llama.cpp sidecar's OpenAI-compatible `/v1/chat/completions` endpoint.
//
// Lightweight Sidecar Strategy:
//   Frames the classification task as a constrained chat completion:
//   sends the user prompt with a system prompt that forces the model to
//   respond with exactly one of the known intent labels plus a confidence
//   score. Parses the structured response.
// =============================================================================

use async_trait::async_trait;

use crate::pipeline::context::IntentClassification;
use crate::pipeline::traits::IntentClassifier;

/// System prompt that instructs the local SLM to act as a Zero-Shot
/// Natural Language Inference (NLI) classifier.
const CLASSIFY_SYSTEM_PROMPT: &str = "\
You are a request classifier for an AI gateway. Analyse the user's prompt and \
classify it into EXACTLY ONE of these categories:\n\n\
- SIMPLE — Greetings, basic factual questions, short answers, simple math.\n\
- COMPLEX — Deep reasoning, multi-step analysis, creative writing, long explanations.\n\
- RAG — Questions that need external documents, knowledge base lookups, or citations.\n\
- CODEGEN — Code generation, debugging, implementation, programming tasks.\n\n\
Reply with EXACTLY this format (no other text):\n\
LABEL: <one of SIMPLE|COMPLEX|RAG|CODEGEN>\n\
CONFIDENCE: <a number between 0.0 and 1.0>\n\n\
Example response:\n\
LABEL: SIMPLE\n\
CONFIDENCE: 0.95";

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
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
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

/// Production intent classifier calling a local llama.cpp sidecar.
pub struct LlamaCppIntentClassifier {
    http_client: reqwest::Client,
    completions_url: String,
    model: String,
}

// Keep old name as alias for ergonomic migration.
pub type OllamaIntentClassifier = LlamaCppIntentClassifier;

impl LlamaCppIntentClassifier {
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

    /// Parse the structured model response into (IntentClassification, confidence).
    fn parse_response(raw: &str) -> (IntentClassification, f64) {
        let upper = raw.to_uppercase();

        // Extract label.
        let label = if let Some(rest) = upper
            .lines()
            .find(|l| l.trim().starts_with("LABEL:"))
        {
            rest.trim()
                .strip_prefix("LABEL:")
                .unwrap_or("")
                .trim()
                .to_string()
        } else {
            // Fallback: look for known keywords anywhere.
            upper.clone()
        };

        let intent = if label.contains("SIMPLE") {
            IntentClassification::Simple
        } else if label.contains("RAG") {
            IntentClassification::Rag
        } else if label.contains("CODEGEN") || label.contains("CODE") {
            IntentClassification::CodeGen
        } else if label.contains("COMPLEX") {
            IntentClassification::Complex
        } else {
            // The model didn't follow instructions — treat as complex for safety.
            IntentClassification::Complex
        };

        // Extract confidence.
        let confidence = upper
            .lines()
            .find(|l| l.trim().starts_with("CONFIDENCE:"))
            .and_then(|line| {
                line.trim()
                    .strip_prefix("CONFIDENCE:")
                    .and_then(|v| v.trim().parse::<f64>().ok())
            })
            .unwrap_or(0.70) // Default confidence if parsing fails.
            .clamp(0.0, 1.0);

        (intent, confidence)
    }
}

#[async_trait]
impl IntentClassifier for LlamaCppIntentClassifier {
    async fn classify(
        &self,
        text: &str,
    ) -> anyhow::Result<(IntentClassification, f64)> {
        let req = ChatCompletionRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: CLASSIFY_SYSTEM_PROMPT.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: text.to_string(),
                },
            ],
            temperature: Some(0.0), // Deterministic classification.
            max_tokens: Some(50),   // Label + confidence only.
            stream: false,
        };

        let resp = self
            .http_client
            .post(&self.completions_url)
            .json(&req)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("LlamaCpp classify request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("LlamaCpp classify returned {status}: {body}");
        }

        let completion: ChatCompletionResponse = resp
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse LlamaCpp classify response: {e}"))?;

        let raw_content = completion
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .unwrap_or_default();

        let (intent, confidence) = Self::parse_response(&raw_content);

        tracing::debug!(
            intent = %intent,
            confidence = format!("{confidence:.3}"),
            raw_response = %raw_content.trim(),
            model = %self.model,
            "LlamaCppIntentClassifier: classified"
        );

        Ok((intent, confidence))
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::context::IntentClassification;
    use crate::pipeline::traits::IntentClassifier;
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

    // ── parse_response unit tests ────────────────────────────────

    #[test]
    fn parse_simple() {
        let (intent, confidence) = LlamaCppIntentClassifier::parse_response(
            "LABEL: SIMPLE\nCONFIDENCE: 0.95",
        );
        assert_eq!(intent, IntentClassification::Simple);
        assert!((confidence - 0.95).abs() < 1e-6);
    }

    #[test]
    fn parse_complex() {
        let (intent, confidence) = LlamaCppIntentClassifier::parse_response(
            "LABEL: COMPLEX\nCONFIDENCE: 0.88",
        );
        assert_eq!(intent, IntentClassification::Complex);
        assert!((confidence - 0.88).abs() < 1e-6);
    }

    #[test]
    fn parse_rag() {
        let (intent, _) = LlamaCppIntentClassifier::parse_response("LABEL: RAG\nCONFIDENCE: 0.9");
        assert_eq!(intent, IntentClassification::Rag);
    }

    #[test]
    fn parse_codegen() {
        let (intent, _) = LlamaCppIntentClassifier::parse_response(
            "LABEL: CODEGEN\nCONFIDENCE: 0.85",
        );
        assert_eq!(intent, IntentClassification::CodeGen);
    }

    #[test]
    fn parse_code_variant() {
        let (intent, _) = LlamaCppIntentClassifier::parse_response(
            "LABEL: CODE\nCONFIDENCE: 0.80",
        );
        assert_eq!(intent, IntentClassification::CodeGen);
    }

    #[test]
    fn parse_missing_confidence_defaults() {
        let (intent, confidence) = LlamaCppIntentClassifier::parse_response("LABEL: SIMPLE");
        assert_eq!(intent, IntentClassification::Simple);
        assert!((confidence - 0.70).abs() < 1e-6);
    }

    #[test]
    fn parse_unrecognized_label_defaults_to_complex() {
        let (intent, _) = LlamaCppIntentClassifier::parse_response(
            "LABEL: FOOBAR\nCONFIDENCE: 0.5",
        );
        assert_eq!(intent, IntentClassification::Complex);
    }

    #[test]
    fn parse_no_label_prefix_fallback() {
        let (intent, _) = LlamaCppIntentClassifier::parse_response("SIMPLE");
        assert_eq!(intent, IntentClassification::Simple);
    }

    #[test]
    fn parse_confidence_clamped() {
        let (_, confidence) = LlamaCppIntentClassifier::parse_response(
            "LABEL: SIMPLE\nCONFIDENCE: 1.5",
        );
        assert!((confidence - 1.0).abs() < 1e-6);
    }

    #[test]
    fn parse_empty_response() {
        let (intent, confidence) = LlamaCppIntentClassifier::parse_response("");
        assert_eq!(intent, IntentClassification::Complex);
        assert!((confidence - 0.70).abs() < 1e-6);
    }

    // ── Constructor ──────────────────────────────────────────────

    #[test]
    fn constructor_appends_path() {
        let client = reqwest::Client::new();
        let c = LlamaCppIntentClassifier::new(client, "http://localhost:8081", "model".into());
        assert_eq!(c.completions_url, "http://localhost:8081/v1/chat/completions");
    }

    #[test]
    fn constructor_no_double_path() {
        let client = reqwest::Client::new();
        let c = LlamaCppIntentClassifier::new(
            client,
            "http://localhost:8081/v1/chat/completions",
            "model".into(),
        );
        assert_eq!(c.completions_url, "http://localhost:8081/v1/chat/completions");
    }

    // ── Integration tests with wiremock ──────────────────────────

    #[tokio::test]
    async fn classify_success() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(mock_response("LABEL: SIMPLE\nCONFIDENCE: 0.95")),
            )
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let classifier =
            LlamaCppIntentClassifier::new(client, &server.uri(), "test-model".into());
        let (intent, confidence) = classifier.classify("hello").await.unwrap();
        assert_eq!(intent, IntentClassification::Simple);
        assert!((confidence - 0.95).abs() < 1e-6);
    }

    #[tokio::test]
    async fn classify_server_error() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let classifier =
            LlamaCppIntentClassifier::new(client, &server.uri(), "test-model".into());
        let result = classifier.classify("hello").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn classify_empty_choices() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"choices": []})),
            )
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let classifier =
            LlamaCppIntentClassifier::new(client, &server.uri(), "test-model".into());
        let (intent, _) = classifier.classify("hello").await.unwrap();
        // Empty content → fallback to Complex.
        assert_eq!(intent, IntentClassification::Complex);
    }
}
