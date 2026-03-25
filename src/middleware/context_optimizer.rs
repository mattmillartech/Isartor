// ═════════════════════════════════════════════════════════════════════
// Layer 2.5 — Context Optimizer Middleware
// ═════════════════════════════════════════════════════════════════════
//
// Sits between L2 SLM triage and L3 cloud handler.  Compresses
// instruction payloads (CLAUDE.md, copilot-instructions.md, skills
// blocks, etc.) so that L3 cloud calls use fewer input tokens.
//
// Only modifies requests that are being forwarded to L3 — L1 cache
// hits and L2 short-circuits never reach this middleware.

use std::sync::Arc;

use axum::{body::Body, extract::Request, middleware::Next, response::Response};
use bytes::Bytes;
use tracing::{Instrument, info_span};

use crate::core::cache_scope::extract_session_cache_scope;
use crate::core::context_compress::optimize_request_body;
use crate::middleware::body_buffer::BufferedBody;
use crate::state::AppState;

/// Layer 2.5 — Context optimizer middleware.
///
/// Detects large instruction/system-message payloads and applies:
///  1. Session dedup  — replace repeat instructions with a hash reference.
///  2. Static minify  — strip comments, whitespace, decorative markers.
///
/// The modified body replaces the `BufferedBody` extension and the
/// request body stream so that L3 handlers receive the compressed
/// version transparently.
pub async fn context_optimizer_middleware(request: Request, next: Next) -> Response {
    let span = info_span!(
        "layer2_5_context_optimizer",
        context.bytes_saved = tracing::field::Empty,
        context.strategy = tracing::field::Empty,
    );

    async move {
        // ── 0. Feature gate ───────────────────────────────────────────
        let state = match request.extensions().get::<Arc<AppState>>() {
            Some(s) => s.clone(),
            None => return next.run(request).await,
        };

        if !state.config.enable_context_optimizer {
            return next.run(request).await;
        }

        // ── 1. Read buffered body ─────────────────────────────────────
        let body_bytes = match request.extensions().get::<BufferedBody>() {
            Some(b) => b.0.clone(),
            None => return next.run(request).await,
        };

        // ── 2. Extract session scope for dedup ────────────────────────
        let session_scope = extract_session_cache_scope(request.headers(), &body_bytes);

        // ── 3. Optimize ───────────────────────────────────────────────
        let result = optimize_request_body(
            &body_bytes,
            session_scope.as_deref(),
            &state.instruction_cache,
            state.config.context_optimizer_dedup,
            state.config.context_optimizer_minify,
        );

        if !result.modified {
            return next.run(request).await;
        }

        // ── 4. Record telemetry ───────────────────────────────────────
        tracing::Span::current().record("context.bytes_saved", result.bytes_saved as u64);
        tracing::Span::current().record("context.strategy", result.strategy.as_str());
        tracing::info!(
            bytes_saved = result.bytes_saved,
            strategy = result.strategy.as_str(),
            "L2.5: compressed instruction context"
        );

        // ── 5. Replace body ───────────────────────────────────────────
        let new_bytes: Bytes = result.body;
        let (mut parts, _old_body) = request.into_parts();
        parts.extensions.insert(BufferedBody(new_bytes.clone()));
        let request = Request::from_parts(parts, Body::from(new_bytes));

        // ── 6. Add response header for observability ──────────────────
        let mut response = next.run(request).await;
        response.headers_mut().insert(
            "x-isartor-context-optimized",
            format!("bytes_saved={}", result.bytes_saved)
                .parse()
                .unwrap(),
        );
        response
    }
    .instrument(span)
    .await
}
