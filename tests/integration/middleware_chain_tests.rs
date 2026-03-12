// =============================================================================
// tests/integration/middleware_chain_tests.rs
//
// Integration tests exercising the complete middleware stack:
//   state injection → body buffer → monitoring → auth → cache → SLM triage → handler
//
// Covers: auth rejection, body survival, concurrent requests, header propagation.
// =============================================================================

use axum::body::Body;
use axum::extract::Request;
use http_body_util::BodyExt;
use tower::ServiceExt;

use crate::common;
use crate::common::*;

// ═══════════════════════════════════════════════════════════════════════
// Auth Tests
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn auth_rejects_missing_api_key() {
    let state = echo_state("http://127.0.0.1:1");
    let app = common::gateway::full_gateway(state);

    let req = Request::builder()
        .method("POST")
        .uri("/api/chat")
        .header("content-type", "application/json")
        .body(json_body("hello"))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn auth_rejects_wrong_api_key() {
    let state = echo_state("http://127.0.0.1:1");
    let app = common::gateway::full_gateway(state);

    let req = Request::builder()
        .method("POST")
        .uri("/api/chat")
        .header("content-type", "application/json")
        .header("X-API-Key", "wrong-key")
        .body(json_body("hello"))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn auth_accepts_valid_api_key() {
    let state = echo_state("http://127.0.0.1:1");
    let app = common::gateway::full_gateway(state);

    let req = Request::builder()
        .method("POST")
        .uri("/api/chat")
        .header("content-type", "application/json")
        .header("X-API-Key", "test-key")
        .body(json_body("hello"))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);
}

// ═══════════════════════════════════════════════════════════════════════
// Body Survival Tests
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn body_survives_full_middleware_chain() {
    let state = echo_state("http://127.0.0.1:1");
    let app = common::gateway::full_gateway(state);

    let prompt = "unique-body-survival-test-12345";
    let req = Request::builder()
        .method("POST")
        .uri("/api/chat")
        .header("content-type", "application/json")
        .header("X-API-Key", "test-key")
        .body(json_body(prompt))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // EchoAgent proves the body made it through all layers.
    assert_eq!(json["layer"], 3);
    assert_eq!(json["message"], format!("echo: {prompt}"));
}

#[tokio::test]
async fn raw_string_body_survives() {
    let state = echo_state("http://127.0.0.1:1");
    let app = common::gateway::full_gateway(state);

    let req = Request::builder()
        .method("POST")
        .uri("/api/chat")
        .header("X-API-Key", "test-key")
        .body(Body::from("raw text prompt"))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["message"], "echo: raw text prompt");
}

// ═══════════════════════════════════════════════════════════════════════
// Concurrency Tests
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn concurrent_requests_all_succeed() {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    let counter = Arc::new(AtomicU32::new(0));
    let agent = CountingAgent {
        response: "counted".into(),
        counter: counter.clone(),
    };

    let config = test_config_exact("http://127.0.0.1:1");
    let embedder =
        Arc::new(isartor::layer1::embeddings::TextEmbedder::new().expect("TextEmbedder init"));
    let state = build_state(Arc::new(agent), config, embedder);

    let n = 10;
    let mut handles = Vec::with_capacity(n);

    for i in 0..n {
        let app = common::gateway::full_gateway(state.clone());
        handles.push(tokio::spawn(async move {
            let req = Request::builder()
                .method("POST")
                .uri("/api/chat")
                .header("content-type", "application/json")
                .header("X-API-Key", "test-key")
                .body(json_body(&format!("concurrent-{i}")))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            resp.status()
        }));
    }

    for handle in handles {
        let status = handle.await.unwrap();
        assert_eq!(status, 200);
    }

    // All requests should have reached Layer 3 (each unique prompt = cache miss).
    assert_eq!(counter.load(Ordering::SeqCst), n as u32);
}

// ═══════════════════════════════════════════════════════════════════════
// Response Format Tests
// ═══════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn response_is_valid_chat_response_json() {
    let state = echo_state("http://127.0.0.1:1");
    let app = common::gateway::full_gateway(state);

    let req = Request::builder()
        .method("POST")
        .uri("/api/chat")
        .header("content-type", "application/json")
        .header("X-API-Key", "test-key")
        .body(json_body("validate format"))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Verify ChatResponse schema: { layer, message, model? }
    assert!(json["layer"].is_number());
    assert!(json["message"].is_string());
    // model can be null or a string
    assert!(json["model"].is_null() || json["model"].is_string());
}
