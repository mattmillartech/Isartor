use std::sync::Arc;

use axum::extract::Request;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use bytes::Bytes;
use http_body_util::BodyExt;
use tracing::{info_span, Instrument};

use crate::models::{ChatResponse, FinalLayer};
use crate::state::AppState;

/// Layer 3 — Fallback handler.
///
/// Runs **only** if every preceding middleware layer decided it could
/// not handle the request. Dispatches the prompt to the configured
/// LLM provider via `rig-core`.
pub async fn chat_handler(request: Request) -> impl IntoResponse {
    let span = info_span!("layer3_llm", ai.prompt.length_bytes = tracing::field::Empty);
    async move {
        let state = request
            .extensions()
            .get::<Arc<AppState>>()
            .expect("AppState missing from request extensions")
            .clone();

        // ------------------------------------------------------------------
    // 1. Read the body (may have been re-attached by Layer 2).
    // ------------------------------------------------------------------
    let body_bytes: Bytes = match request.into_body().collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ChatResponse {
                    layer: 3,
                    message: "Failed to read request body".into(),
                    model: None,
                }),
            )
                .into_response();
        }
    };

    // Try JSON `{ "prompt": "..." }`, fall back to raw string.
    let prompt: String = serde_json::from_slice::<serde_json::Value>(&body_bytes)
        .ok()
        .and_then(|v| v.get("prompt").and_then(|p| p.as_str()).map(String::from))
        .unwrap_or_else(|| String::from_utf8_lossy(&body_bytes).to_string());

    tracing::Span::current().record("ai.prompt.length_bytes", prompt.len() as u64);

    let provider_name = state.llm_agent.provider_name();
    tracing::info!(prompt = %prompt, provider = provider_name, "Layer 3: Forwarding to Layer 3 LLM via Rig");

    // ------------------------------------------------------------------
    // 2. Dispatch to the configured rig-core Agent.
    // ------------------------------------------------------------------
    match state.llm_agent.chat(&prompt).await {
        Ok(text) => {
            let mut response = (
                StatusCode::OK,
                Json(ChatResponse {
                    layer: 3,
                    message: text,
                    model: Some(state.config.external_llm_model.clone()),
                }),
            )
                .into_response();
            response.extensions_mut().insert(FinalLayer::Cloud);
            response
        }
        Err(e) => {
            tracing::error!(error = %e, provider = provider_name, "Layer 3: LLM call failed");
            let mut response = (
                StatusCode::BAD_GATEWAY,
                Json(ChatResponse {
                    layer: 3,
                    message: format!("[{provider_name}] {e}"),
                    model: None,
                }),
            )
                .into_response();
            response.extensions_mut().insert(FinalLayer::Cloud);
            response
        }
    }
    }
    .instrument(span)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, middleware as axum_mw, routing::post, Router};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use crate::clients::slm::SlmClient;
    use crate::config::{AppConfig, CacheMode, EmbeddingSidecarSettings, Layer2Settings};
    use crate::layer1::embeddings::shared_test_embedder;
    use crate::state::{AppLlmAgent, ExactCache};
    use crate::vector_cache::VectorCache;

    struct SuccessAgent;

    #[async_trait::async_trait]
    impl AppLlmAgent for SuccessAgent {
        async fn chat(&self, prompt: &str) -> anyhow::Result<String> {
            Ok(format!("Reply to: {prompt}"))
        }
        fn provider_name(&self) -> &'static str {
            "mock"
        }
    }

    struct FailAgent;

    #[async_trait::async_trait]
    impl AppLlmAgent for FailAgent {
        async fn chat(&self, _prompt: &str) -> anyhow::Result<String> {
            Err(anyhow::anyhow!("provider outage"))
        }
        fn provider_name(&self) -> &'static str {
            "mock"
        }
    }

    fn test_state(agent: Arc<dyn AppLlmAgent>) -> Arc<AppState> {
        let config = Arc::new(AppConfig {
            host_port: "127.0.0.1:0".into(),
            inference_engine: crate::config::InferenceEngineMode::Sidecar,
            gateway_api_key: "test".into(),
            cache_mode: CacheMode::Exact,
            embedding_model: "all-minilm".into(),
            similarity_threshold: 0.85,
            cache_ttl_secs: 300,
            cache_max_capacity: 100,
            layer2: Layer2Settings {
                sidecar_url: "http://127.0.0.1:8081".into(),
                model_name: "test".into(),
                timeout_seconds: 5,
            },
            local_slm_url: "http://localhost:11434/api/generate".into(),
            local_slm_model: "llama3".into(),
            embedding_sidecar: EmbeddingSidecarSettings {
                sidecar_url: "http://127.0.0.1:8082".into(),
                model_name: "test".into(),
                timeout_seconds: 5,
            },
            llm_provider: "openai".into(),
            external_llm_url: "http://localhost".into(),
            external_llm_model: "gpt-4o-mini".into(),
            external_llm_api_key: "".into(),
            azure_deployment_id: "".into(),
            azure_api_version: "".into(),
            enable_monitoring: false,
            otel_exporter_endpoint: "http://localhost:4317".into(),
            pipeline_embedding_dim: 384,
            pipeline_similarity_threshold: 0.92,
            pipeline_rerank_top_k: 5,
            pipeline_max_concurrency: 256,
            pipeline_min_concurrency: 4,
            pipeline_target_latency_ms: 500,
        });

        Arc::new(AppState {
            http_client: reqwest::Client::new(),
            exact_cache: Arc::new(ExactCache::new(300, 100)),
            vector_cache: Arc::new(VectorCache::new(0.85, 300, 100)),
            llm_agent: agent,
            slm_client: Arc::new(SlmClient::new(&config.layer2)),
            text_embedder: shared_test_embedder(),
            config,
            #[cfg(feature = "embedded-inference")]
            embedded_classifier: None,
        })
    }

    fn handler_app(state: Arc<AppState>) -> Router {
        Router::new()
            .route("/api/chat", post(chat_handler))
            .layer(axum_mw::from_fn(
                move |mut req: Request, next: axum_mw::Next| {
                    let st = state.clone();
                    async move {
                        req.extensions_mut().insert(st);
                        next.run(req).await
                    }
                },
            ))
    }

    #[tokio::test]
    async fn successful_llm_response() {
        let state = test_state(Arc::new(SuccessAgent));
        let app = handler_app(state);

        let body = serde_json::to_vec(&serde_json::json!({ "prompt": "hello" })).unwrap();
        let req = Request::builder()
            .method("POST")
            .uri("/api/chat")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(json["layer"], 3);
        assert_eq!(json["message"], "Reply to: hello");
        assert_eq!(json["model"], "gpt-4o-mini");
    }

    #[tokio::test]
    async fn llm_failure_returns_502() {
        let state = test_state(Arc::new(FailAgent));
        let app = handler_app(state);

        let body = serde_json::to_vec(&serde_json::json!({ "prompt": "test" })).unwrap();
        let req = Request::builder()
            .method("POST")
            .uri("/api/chat")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);

        let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(json["layer"], 3);
        assert!(json["message"]
            .as_str()
            .unwrap()
            .contains("provider outage"));
    }

    #[tokio::test]
    async fn raw_string_body_used_as_prompt() {
        let state = test_state(Arc::new(SuccessAgent));
        let app = handler_app(state);

        let req = Request::builder()
            .method("POST")
            .uri("/api/chat")
            .body(Body::from("raw text prompt"))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(json["message"], "Reply to: raw text prompt");
    }
}
