use std::sync::Arc;
use std::time::Instant;

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::IntoResponse;
use opentelemetry::{global, KeyValue};
use opentelemetry::metrics::Counter;
use tracing::{info_span, Instrument};

use crate::state::AppState;

/// Root monitoring middleware.
///
/// Starts the parent trace span for the request, yielding performance metrics.
/// On completion, it records a standard `gateway_requests_total` metric
/// with labels indicating the final layer that handled the query.
pub async fn root_monitoring_middleware(
    request: Request,
    next: Next,
) -> impl IntoResponse {
    let method = request.method().to_string();
    let path = request.uri().path().to_string();

    let root_span = info_span!(
        "gateway_request",
        method = %method,
        path = %path,
        gateway.final_handling_layer = tracing::field::Empty,
    );

    async {
        let state = request
            .extensions()
            .get::<Arc<AppState>>()
            .expect("AppState missing")
            .clone();

        let enable_monitoring = state.config.enable_monitoring;
        let mut meter_counter: Option<Counter<u64>> = None;

        if enable_monitoring {
            // Get or create the custom metric counter
            let meter = global::meter("isartor.gateway");
            meter_counter = Some(
                meter
                    .u64_counter("gateway_requests_total")
                    .with_description("Total requests processed by the AI orchestration gateway")
                    .build(),
            );
        }

        let start_time = Instant::now();

        // Pass the request down the pipeline
        let response = next.run(request).await;

        let duration = start_time.elapsed();

        // Attempt to extract the "final_handling_layer" attribute from the span 
        // to categorize cost. This must be set by the child layers.
        // For metrics, we need to extract exactly what the child layers reported via extensions or headers.
        
        let mut handled_by_layer = "unknown";
        if let Some(extensions) = response.extensions().get::<crate::models::ChatResponse>() {
            handled_by_layer = match extensions.layer {
                0 => "auth_blocked",
                1 => "cache",
                2 => "slm",
                3 => "llm",
                _ => "unknown",
            };
        } else if response.status().is_client_error() {
            handled_by_layer = "auth_blocked";
        }

        // Record trace span attribute
        root_span.record("gateway.final_handling_layer", handled_by_layer);

        // Record OTel Metrics Counter
        if let Some(counter) = meter_counter {
            counter.add(
                1,
                &[
                    KeyValue::new("handled_by", handled_by_layer),
                    KeyValue::new("status_code", response.status().as_u16().to_string()),
                ],
            );
        }

        tracing::info!(
            duration_ms = duration.as_millis(),
            handled_by = handled_by_layer,
            status = response.status().as_u16(),
            "Request finished"
        );

        response
    }
    .instrument(root_span.clone())
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, routing::post, Router, middleware as axum_mw, Json};
    use axum::http::StatusCode;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use crate::clients::slm::SlmClient;
    use crate::config::{AppConfig, CacheMode, Layer2Settings, EmbeddingSidecarSettings};
    use crate::models::ChatResponse;
    use crate::state::{AppLlmAgent, ExactCache};
    use crate::vector_cache::VectorCache;

    struct MockAgent;

    #[async_trait::async_trait]
    impl AppLlmAgent for MockAgent {
        async fn chat(&self, _prompt: &str) -> anyhow::Result<String> {
            Ok("mock".into())
        }
        fn provider_name(&self) -> &'static str {
            "mock"
        }
    }

    fn test_state(enable_monitoring: bool) -> Arc<AppState> {
        let config = Arc::new(AppConfig {
            host_port: "127.0.0.1:0".into(),
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
            external_llm_model: "test".into(),
            external_llm_api_key: "".into(),
            azure_deployment_id: "".into(),
            azure_api_version: "".into(),
            enable_monitoring,
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
            llm_agent: Arc::new(MockAgent),
            slm_client: Arc::new(SlmClient::new(&config.layer2)),
            config,
        })
    }

    fn monitoring_app(state: Arc<AppState>) -> Router {
        Router::new()
            .route("/api/chat", post(|| async {
                (StatusCode::OK, Json(ChatResponse {
                    layer: 3,
                    message: "ok".into(),
                    model: None,
                }))
            }))
            .layer(axum_mw::from_fn(root_monitoring_middleware))
            .layer(axum_mw::from_fn(move |mut req: axum::extract::Request, next: axum_mw::Next| {
                let st = state.clone();
                async move {
                    req.extensions_mut().insert(st);
                    next.run(req).await
                }
            }))
    }

    #[tokio::test]
    async fn monitoring_passes_through_disabled() {
        let state = test_state(false);
        let app = monitoring_app(state);

        let req = axum::extract::Request::builder()
            .method("POST")
            .uri("/api/chat")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["layer"], 3);
    }

    #[tokio::test]
    async fn monitoring_passes_through_enabled() {
        let state = test_state(true);
        let app = monitoring_app(state);

        let req = axum::extract::Request::builder()
            .method("POST")
            .uri("/api/chat")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn monitoring_records_client_error_as_auth_blocked() {
        let state = test_state(false);
        // Build an app that returns 401
        let app = Router::new()
            .route("/api/chat", post(|| async {
                StatusCode::UNAUTHORIZED.into_response()
            }))
            .layer(axum_mw::from_fn(root_monitoring_middleware))
            .layer(axum_mw::from_fn(move |mut req: axum::extract::Request, next: axum_mw::Next| {
                let st = state.clone();
                async move {
                    req.extensions_mut().insert(st);
                    next.run(req).await
                }
            }));

        let req = axum::extract::Request::builder()
            .method("POST")
            .uri("/api/chat")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
