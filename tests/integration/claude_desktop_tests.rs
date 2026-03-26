use std::sync::Arc;

use axum::{
    Router,
    extract::Request,
    middleware as axum_mw,
    routing::{get, post},
};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

use crate::common;
use isartor::handler::{
    cache_lookup_handler, cache_store_handler, mcp_http_delete_handler, mcp_http_get_handler,
    mcp_http_post_handler,
};
use isartor::mcp;

fn mcp_gateway(state: Arc<isartor::state::AppState>) -> Router {
    let state_for_ext = state.clone();
    Router::new()
        .route("/api/v1/cache/lookup", post(cache_lookup_handler))
        .route("/api/v1/cache/store", post(cache_store_handler))
        .route(
            "/mcp/",
            get(mcp_http_get_handler)
                .post(mcp_http_post_handler)
                .delete(mcp_http_delete_handler),
        )
        .route(
            "/mcp",
            get(mcp_http_get_handler)
                .post(mcp_http_post_handler)
                .delete(mcp_http_delete_handler),
        )
        .layer(axum_mw::from_fn(
            move |mut req: Request, next: axum_mw::Next| {
                let st = state_for_ext.clone();
                async move {
                    req.extensions_mut().insert(st);
                    next.run(req).await
                }
            },
        ))
}

#[tokio::test]
async fn claude_desktop_mcp_http_flow_lists_tools_and_round_trips_cache() {
    let state = common::echo_state("http://127.0.0.1:9");
    let app = mcp_gateway(state);

    let initialize_req = Request::builder()
        .method("POST")
        .uri("/mcp/")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {}
            }))
            .unwrap(),
        ))
        .unwrap();
    let initialize_resp = app.clone().oneshot(initialize_req).await.unwrap();
    assert_eq!(initialize_resp.status(), 200);
    let session_id = initialize_resp
        .headers()
        .get(mcp::SESSION_HEADER)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let tools_req = Request::builder()
        .method("POST")
        .uri("/mcp/")
        .header("content-type", "application/json")
        .header(mcp::SESSION_HEADER, &session_id)
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/list"
            }))
            .unwrap(),
        ))
        .unwrap();
    let tools_resp = app.clone().oneshot(tools_req).await.unwrap();
    assert_eq!(tools_resp.status(), 200);
    let tools_body = tools_resp.into_body().collect().await.unwrap().to_bytes();
    let tools_json: Value = serde_json::from_slice(&tools_body).unwrap();
    let tools = tools_json["result"]["tools"].as_array().unwrap();
    assert!(tools.iter().any(|tool| tool["name"] == "isartor_chat"));
    assert!(
        tools
            .iter()
            .any(|tool| tool["name"] == "isartor_cache_store")
    );

    let store_req = Request::builder()
        .method("POST")
        .uri("/mcp/")
        .header("content-type", "application/json")
        .header(mcp::SESSION_HEADER, &session_id)
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": {
                    "name": "isartor_cache_store",
                    "arguments": {
                        "prompt": "What is Isartor?",
                        "response": "Isartor is a prompt firewall.",
                        "model": "claude-sonnet-4-6"
                    }
                }
            }))
            .unwrap(),
        ))
        .unwrap();
    let store_resp = app.clone().oneshot(store_req).await.unwrap();
    assert_eq!(store_resp.status(), 200);

    let lookup_req = Request::builder()
        .method("POST")
        .uri("/mcp/")
        .header("content-type", "application/json")
        .header(mcp::SESSION_HEADER, &session_id)
        .body(axum::body::Body::from(
            serde_json::to_vec(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 4,
                "method": "tools/call",
                "params": {
                    "name": "isartor_chat",
                    "arguments": {
                        "prompt": "What is Isartor?"
                    }
                }
            }))
            .unwrap(),
        ))
        .unwrap();
    let lookup_resp = app.clone().oneshot(lookup_req).await.unwrap();
    assert_eq!(lookup_resp.status(), 200);
    let lookup_body = lookup_resp.into_body().collect().await.unwrap().to_bytes();
    let lookup_json: Value = serde_json::from_slice(&lookup_body).unwrap();
    assert_eq!(
        lookup_json["result"]["content"][0]["text"],
        "Isartor is a prompt firewall."
    );

    let delete_req = Request::builder()
        .method("DELETE")
        .uri("/mcp/")
        .header(mcp::SESSION_HEADER, &session_id)
        .body(axum::body::Body::empty())
        .unwrap();
    let delete_resp = app.oneshot(delete_req).await.unwrap();
    assert_eq!(delete_resp.status(), 204);
}
