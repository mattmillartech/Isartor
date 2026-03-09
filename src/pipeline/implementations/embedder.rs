// =============================================================================
// LlamaCppEmbedder — Layer 1 Embedder backed by a llama.cpp sidecar's
// OpenAI-compatible `/v1/embeddings` endpoint.
//
// Converts text into dense embedding vectors using a locally-running
// llama.cpp server with `--embedding` enabled, hosting a quantised
// embedding model (e.g., all-MiniLM-L6-v2 in GGUF format).
//
// Lightweight Sidecar Strategy:
//   The embedding sidecar runs as a separate llama.cpp instance on a
//   dedicated port, configured via `EmbeddingSidecarSettings`.
// =============================================================================

use async_trait::async_trait;

use crate::pipeline::traits::Embedder;

/// Production embedder that calls the llama.cpp sidecar's `/v1/embeddings`.
pub struct LlamaCppEmbedder {
    /// HTTP client (with timeout from EmbeddingSidecarSettings).
    http_client: reqwest::Client,

    /// Full URL of the embeddings endpoint (e.g. "http://127.0.0.1:8082/v1/embeddings").
    embeddings_url: String,

    /// Model name for the API request body (informational for llama.cpp).
    model: String,

    /// Expected embedding dimensionality (for validation / observability).
    dimension: usize,
}

// Keep the old name as a type alias for ergonomic migration.
pub type OllamaEmbedder = LlamaCppEmbedder;

impl LlamaCppEmbedder {
    pub fn new(
        http_client: reqwest::Client,
        sidecar_base_url: &str,
        model: String,
        dimension: usize,
    ) -> Self {
        let base = sidecar_base_url.trim_end_matches('/');
        let embeddings_url = if base.ends_with("/v1/embeddings") {
            base.to_string()
        } else {
            format!("{base}/v1/embeddings")
        };

        Self {
            http_client,
            embeddings_url,
            model,
            dimension,
        }
    }
}

/// OpenAI-compatible embedding request body.
#[derive(serde::Serialize)]
struct EmbeddingRequest {
    model: String,
    input: String,
}

/// A single embedding object in the response.
#[derive(serde::Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

/// OpenAI-compatible embedding response body.
#[derive(serde::Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[async_trait]
impl Embedder for LlamaCppEmbedder {
    async fn embed(&self, text: &str) -> anyhow::Result<Vec<f64>> {
        let req = EmbeddingRequest {
            model: self.model.clone(),
            input: text.to_string(),
        };

        let resp = self
            .http_client
            .post(&self.embeddings_url)
            .json(&req)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("LlamaCpp embedding request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("LlamaCpp embedding returned {status}: {body}");
        }

        let embed_resp: EmbeddingResponse = resp
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse LlamaCpp embedding response: {e}"))?;

        let raw = embed_resp
            .data
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("LlamaCpp returned empty embeddings data array"))?
            .embedding;

        if raw.is_empty() {
            anyhow::bail!("LlamaCpp returned a zero-length embedding vector");
        }

        // Convert f32 → f64 and L2-normalise.
        let mut vector: Vec<f64> = raw.into_iter().map(|v| v as f64).collect();

        // Validate dimension if configured.
        if self.dimension > 0 && vector.len() != self.dimension {
            tracing::warn!(
                expected = self.dimension,
                actual = vector.len(),
                "LlamaCppEmbedder: dimension mismatch"
            );
        }

        let magnitude: f64 = vector.iter().map(|x| x * x).sum::<f64>().sqrt();
        if magnitude > 0.0 {
            for v in &mut vector {
                *v /= magnitude;
            }
        }

        tracing::debug!(
            dims = vector.len(),
            model = %self.model,
            "LlamaCppEmbedder: embedding computed"
        );

        Ok(vector)
    }

    fn embedding_dimension(&self) -> usize {
        self.dimension
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::traits::Embedder;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn make_embedder(url: &str) -> LlamaCppEmbedder {
        let client = reqwest::Client::new();
        LlamaCppEmbedder::new(client, url, "test-model".into(), 3)
    }

    #[test]
    fn constructor_appends_path() {
        let e = make_embedder("http://localhost:8082");
        assert_eq!(e.embeddings_url, "http://localhost:8082/v1/embeddings");
    }

    #[test]
    fn constructor_does_not_double_path() {
        let e = make_embedder("http://localhost:8082/v1/embeddings");
        assert_eq!(e.embeddings_url, "http://localhost:8082/v1/embeddings");
    }

    #[test]
    fn embedding_dimension_and_model_name() {
        let e = make_embedder("http://localhost:8082");
        assert_eq!(e.embedding_dimension(), 3);
        assert_eq!(e.model_name(), "test-model");
    }

    #[tokio::test]
    async fn embed_success() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{
                    "embedding": [3.0, 4.0, 0.0],
                    "index": 0
                }],
                "model": "test"
            })))
            .mount(&server)
            .await;

        let embedder = make_embedder(&server.uri());
        let vector = embedder.embed("hello").await.unwrap();

        // Should be L2-normalised: 3/5, 4/5, 0
        assert_eq!(vector.len(), 3);
        let magnitude: f64 = vector.iter().map(|x| x * x).sum::<f64>().sqrt();
        assert!((magnitude - 1.0).abs() < 1e-6, "Should be normalised");
    }

    #[tokio::test]
    async fn embed_server_error() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(500).set_body_string("error"))
            .mount(&server)
            .await;

        let embedder = make_embedder(&server.uri());
        let result = embedder.embed("hello").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("500"));
    }

    #[tokio::test]
    async fn embed_empty_data_array() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [],
                "model": "test"
            })))
            .mount(&server)
            .await;

        let embedder = make_embedder(&server.uri());
        let result = embedder.embed("hello").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[tokio::test]
    async fn embed_zero_length_embedding() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"embedding": [], "index": 0}],
                "model": "test"
            })))
            .mount(&server)
            .await;

        let embedder = make_embedder(&server.uri());
        let result = embedder.embed("hello").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("zero-length"));
    }

    #[tokio::test]
    async fn embed_malformed_json() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;

        let embedder = make_embedder(&server.uri());
        let result = embedder.embed("hello").await;
        assert!(result.is_err());
    }
}
