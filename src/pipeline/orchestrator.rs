// =============================================================================
// Pipeline Orchestrator — The main execution engine.
//
// This is the heart of the Algorithmic AI Gateway. It sequences every
// layer of the pipeline in strict order:
//
//   Step 0 (Ops)      → Adaptive concurrency check / load shedding
//   Step 1 (Cache)    → Embed prompt → search vector store → short-circuit on hit
//   Step 2 (Router)   → Classify intent → short-circuit simple tasks via local SLM
//   Step 2.5 (Optim)  → Retrieve candidate docs → rerank to top-K
//   Step 3 (Fallback) → Construct augmented prompt → call external LLM
//
// At each step, if the layer produces a final answer, the pipeline
// short-circuits: no downstream layers execute.
// =============================================================================

use std::time::Instant;

use super::concurrency::AdaptiveConcurrencyLimiter;
use super::context::{IntentClassification, PipelineContext, PipelineResponse};
use super::traits::AlgorithmSuite;

/// Runtime configuration for the pipeline orchestrator.
///
/// Extracted from `AppConfig` at startup and threaded through to avoid
/// referencing the global config crate inside the pipeline.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Cosine similarity threshold for semantic cache hits (Layer 1).
    pub similarity_threshold: f64,
    /// Number of top documents to keep after reranking (Layer 2.5).
    pub rerank_top_k: usize,
}

/// The number of mock documents "retrieved" from the knowledge base
/// before reranking (simulates a vector DB / full-text search result).
const MOCK_RETRIEVED_DOCUMENT_COUNT: usize = 20;

// ═════════════════════════════════════════════════════════════════════
// Orchestrator
// ═════════════════════════════════════════════════════════════════════

/// Runs the full algorithmic pipeline for a single request.
///
/// This function is the API-level entry point. It:
/// 1. Creates a `PipelineContext` for the request.
/// 2. Passes the context through each algorithmic layer.
/// 3. Returns a `PipelineResponse` with the result and full processing log.
///
/// The `concurrency_limiter` and `algorithms` are shared across all
/// requests (wrapped in `Arc` at the application level).
pub async fn execute_pipeline(
    prompt: String,
    concurrency_limiter: &AdaptiveConcurrencyLimiter,
    algorithms: &AlgorithmSuite,
    pipeline_cfg: &PipelineConfig,
) -> PipelineResponse {
    let mut ctx = PipelineContext::new(prompt);

    tracing::info!(
        request_id = %ctx.request_id,
        prompt_len = ctx.original_prompt.len(),
        "Pipeline: request received"
    );

    // ──────────────────────────────────────────────────────────────
    // Step 0 — Adaptive Concurrency (Load Shedding)
    // ──────────────────────────────────────────────────────────────
    let step0_start = Instant::now();

    let permit = match concurrency_limiter.try_acquire() {
        Ok(permit) => {
            ctx.log_step(
                "Layer0_AdaptiveConcurrency",
                "Permit acquired",
                step0_start.elapsed().as_millis() as u64,
                false,
                Some(serde_json::json!({
                    "in_flight": concurrency_limiter.current_in_flight(),
                    "limit": concurrency_limiter.current_limit(),
                })),
            );
            tracing::debug!(
                in_flight = concurrency_limiter.current_in_flight(),
                limit = concurrency_limiter.current_limit(),
                "Step 0: Concurrency permit acquired"
            );
            permit
        }
        Err(()) => {
            // System overloaded — reject immediately (load shedding).
            ctx.log_step(
                "Layer0_AdaptiveConcurrency",
                "REJECTED — system overloaded",
                step0_start.elapsed().as_millis() as u64,
                true,
                Some(serde_json::json!({
                    "in_flight": concurrency_limiter.current_in_flight(),
                    "limit": concurrency_limiter.current_limit(),
                })),
            );
            tracing::warn!(
                in_flight = concurrency_limiter.current_in_flight(),
                limit = concurrency_limiter.current_limit(),
                "Step 0: Request SHED — concurrency limit exceeded"
            );
            return PipelineResponse {
                resolved_by_layer: 0,
                message: "Service overloaded. Please retry later.".into(),
                model: None,
                total_duration_ms: ctx.elapsed_ms(),
                processing_log: ctx.processing_log,
            };
        }
    };

    // ──────────────────────────────────────────────────────────────
    // Step 1 — Semantic Cache (Embed + Vector Search)
    // ──────────────────────────────────────────────────────────────
    let step1_start = Instant::now();

    // Step 1a: Compute the embedding vector.
    match algorithms.embedder.embed(&ctx.original_prompt).await {
        Ok(vector) => {
            tracing::debug!(
                dims = vector.len(),
                model = algorithms.embedder.model_name(),
                "Step 1: Embedding computed"
            );
            ctx.request_vector = Some(vector);
        }
        Err(e) => {
            tracing::warn!(error = %e, "Step 1: Embedding failed — skipping cache layer");
            ctx.log_step(
                "Layer1_Embed",
                format!("Embedding failed: {e}"),
                step1_start.elapsed().as_millis() as u64,
                false,
                None,
            );
            // Continue to Layer 2 — cache is non-critical.
        }
    }

    // Step 1b: Search the vector store (only if we have an embedding).
    if let Some(ref query_vec) = ctx.request_vector {
        match algorithms
            .vector_store
            .search(query_vec, pipeline_cfg.similarity_threshold)
            .await
        {
            Ok(Some((cached_response, similarity))) => {
                ctx.log_step(
                    "Layer1_SemanticCache",
                    format!("Cache HIT (similarity={similarity:.4})"),
                    step1_start.elapsed().as_millis() as u64,
                    true,
                    Some(serde_json::json!({
                        "similarity": similarity,
                        "threshold": pipeline_cfg.similarity_threshold,
                        "embedder": algorithms.embedder.model_name(),
                    })),
                );
                tracing::info!(
                    similarity = format!("{similarity:.4}"),
                    "Step 1: Semantic cache HIT — short-circuiting pipeline"
                );
                // Release the concurrency permit and record latency.
                permit.release().await;
                return PipelineResponse {
                    resolved_by_layer: 1,
                    message: cached_response,
                    model: None,
                    total_duration_ms: ctx.elapsed_ms(),
                    processing_log: ctx.processing_log,
                };
            }
            Ok(None) => {
                ctx.log_step(
                    "Layer1_SemanticCache",
                    "Cache MISS",
                    step1_start.elapsed().as_millis() as u64,
                    false,
                    Some(serde_json::json!({
                        "threshold": pipeline_cfg.similarity_threshold,
                        "store_size": algorithms.vector_store.len().await,
                    })),
                );
                tracing::debug!("Step 1: Semantic cache MISS — proceeding to Layer 2");
            }
            Err(e) => {
                ctx.log_step(
                    "Layer1_SemanticCache",
                    format!("Search failed: {e}"),
                    step1_start.elapsed().as_millis() as u64,
                    false,
                    None,
                );
                tracing::warn!(error = %e, "Step 1: Vector search failed — proceeding to Layer 2");
            }
        }
    }

    // ──────────────────────────────────────────────────────────────
    // Step 2 — SLM Router (Intent Classification)
    // ──────────────────────────────────────────────────────────────
    let step2_start = Instant::now();

    match algorithms
        .intent_classifier
        .classify(&ctx.original_prompt)
        .await
    {
        Ok((intent, confidence)) => {
            tracing::info!(
                intent = %intent,
                confidence = format!("{confidence:.3}"),
                model = algorithms.intent_classifier.model_name(),
                "Step 2: Intent classified"
            );
            ctx.intent_classification = intent;
            ctx.complexity_score = confidence;
        }
        Err(e) => {
            tracing::warn!(error = %e, "Step 2: Classification failed — defaulting to Complex");
            ctx.intent_classification = IntentClassification::Unclassifiable;
            ctx.complexity_score = 0.0;
        }
    }

    ctx.log_step(
        "Layer2_IntentClassifier",
        format!(
            "Classified as {} (confidence={:.3})",
            ctx.intent_classification, ctx.complexity_score
        ),
        step2_start.elapsed().as_millis() as u64,
        false,
        Some(serde_json::json!({
            "intent": ctx.intent_classification.to_string(),
            "confidence": ctx.complexity_score,
            "classifier": algorithms.intent_classifier.model_name(),
        })),
    );

    // Step 2 — Route: If Simple, execute locally and short-circuit.
    if ctx.intent_classification == IntentClassification::Simple {
        let exec_start = Instant::now();

        match algorithms
            .local_executor
            .execute_simple(&ctx.original_prompt)
            .await
        {
            Ok(response) => {
                ctx.log_step(
                    "Layer2_LocalExecutor",
                    "Simple task executed by local SLM",
                    exec_start.elapsed().as_millis() as u64,
                    true,
                    Some(serde_json::json!({
                        "executor": algorithms.local_executor.model_name(),
                        "response_len": response.len(),
                    })),
                );
                tracing::info!(
                    model = algorithms.local_executor.model_name(),
                    "Step 2: Simple task resolved locally — short-circuiting pipeline"
                );

                // Cache the result for future semantic matches.
                cache_response(&ctx, &response, algorithms).await;

                permit.release().await;
                return PipelineResponse {
                    resolved_by_layer: 2,
                    message: response,
                    model: Some(algorithms.local_executor.model_name().to_string()),
                    total_duration_ms: ctx.elapsed_ms(),
                    processing_log: ctx.processing_log,
                };
            }
            Err(e) => {
                ctx.log_step(
                    "Layer2_LocalExecutor",
                    format!("Local execution failed: {e}"),
                    exec_start.elapsed().as_millis() as u64,
                    false,
                    None,
                );
                tracing::warn!(error = %e, "Step 2: Local SLM execution failed — falling through to Layer 3");
                // Fall through to Layer 2.5 / 3.
            }
        }
    }

    // ──────────────────────────────────────────────────────────────
    // Step 2.5 — Context Optimiser (Retrieve + Rerank)
    //
    // Only runs for non-simple tasks that will proceed to the
    // external LLM. The goal is to minimise token usage by sending
    // only the most relevant context documents.
    // ──────────────────────────────────────────────────────────────
    let step25_start = Instant::now();

    // Step 2.5a: Simulate retrieving candidate documents from a knowledge base.
    //
    // In production, this would be a call to a vector DB, full-text search
    // engine (Elasticsearch, Meilisearch), or a hybrid retrieval system.
    ctx.retrieved_context_documents = generate_mock_documents(MOCK_RETRIEVED_DOCUMENT_COUNT);

    tracing::debug!(
        doc_count = ctx.retrieved_context_documents.len(),
        "Step 2.5: Retrieved candidate documents from knowledge base"
    );

    // Step 2.5b: Rerank documents to find the top-K most relevant.
    match algorithms
        .reranker
        .rerank(
            &ctx.original_prompt,
            &ctx.retrieved_context_documents,
            pipeline_cfg.rerank_top_k,
        )
        .await
    {
        Ok(reranked) => {
            let scores: Vec<f64> = reranked.iter().map(|(_, s)| *s).collect();
            ctx.optimised_context_documents = reranked.into_iter().map(|(doc, _)| doc).collect();

            ctx.log_step(
                "Layer2.5_ContextOptimiser",
                format!(
                    "Reranked {} → {} documents",
                    ctx.retrieved_context_documents.len(),
                    ctx.optimised_context_documents.len()
                ),
                step25_start.elapsed().as_millis() as u64,
                false,
                Some(serde_json::json!({
                    "input_docs": ctx.retrieved_context_documents.len(),
                    "output_docs": ctx.optimised_context_documents.len(),
                    "reranker": algorithms.reranker.model_name(),
                    "top_scores": scores,
                })),
            );
            tracing::info!(
                input_docs = ctx.retrieved_context_documents.len(),
                output_docs = ctx.optimised_context_documents.len(),
                model = algorithms.reranker.model_name(),
                "Step 2.5: Context optimised via reranking"
            );
        }
        Err(e) => {
            // Reranking failed — fall back to using all retrieved docs (degraded mode).
            let top_k = pipeline_cfg.rerank_top_k;
            ctx.optimised_context_documents = ctx
                .retrieved_context_documents
                .iter()
                .take(top_k)
                .cloned()
                .collect();

            ctx.log_step(
                "Layer2.5_ContextOptimiser",
                format!("Reranking failed: {e} — using first {top_k} docs"),
                step25_start.elapsed().as_millis() as u64,
                false,
                None,
            );
            tracing::warn!(
                error = %e,
                top_k = top_k,
                "Step 2.5: Reranking failed — using unranked top-K docs"
            );
        }
    }

    // ──────────────────────────────────────────────────────────────
    // Step 3 — External LLM Fallback
    //
    // Construct the final payload: original prompt + optimised context
    // documents, and send to the external LLM.
    // ──────────────────────────────────────────────────────────────
    let step3_start = Instant::now();

    tracing::info!(
        provider = algorithms.external_llm.provider_name(),
        model = algorithms.external_llm.model_name(),
        context_docs = ctx.optimised_context_documents.len(),
        "Step 3: Dispatching to external LLM"
    );

    let response = match algorithms
        .external_llm
        .complete(&ctx.original_prompt, &ctx.optimised_context_documents)
        .await
    {
        Ok(text) => {
            ctx.log_step(
                "Layer3_ExternalLLM",
                format!("Completion received ({} chars)", text.len()),
                step3_start.elapsed().as_millis() as u64,
                true,
                Some(serde_json::json!({
                    "provider": algorithms.external_llm.provider_name(),
                    "model": algorithms.external_llm.model_name(),
                    "context_docs": ctx.optimised_context_documents.len(),
                    "response_len": text.len(),
                })),
            );
            tracing::info!(
                response_len = text.len(),
                "Step 3: External LLM response received"
            );
            text
        }
        Err(e) => {
            ctx.log_step(
                "Layer3_ExternalLLM",
                format!("LLM call failed: {e}"),
                step3_start.elapsed().as_millis() as u64,
                true,
                None,
            );
            tracing::error!(error = %e, "Step 3: External LLM call failed");
            format!("Error: External LLM unavailable — {e}")
        }
    };

    // Cache the successful response for future semantic matches.
    cache_response(&ctx, &response, algorithms).await;

    permit.release().await;

    PipelineResponse {
        resolved_by_layer: 3,
        message: response,
        model: Some(algorithms.external_llm.model_name().to_string()),
        total_duration_ms: ctx.elapsed_ms(),
        processing_log: ctx.processing_log,
    }
}

// ═════════════════════════════════════════════════════════════════════
// Internal helpers
// ═════════════════════════════════════════════════════════════════════

/// Store a successful response in the vector cache for future hits.
async fn cache_response(ctx: &PipelineContext, response: &str, algorithms: &AlgorithmSuite) {
    if let Some(ref vector) = ctx.request_vector {
        if let Err(e) = algorithms
            .vector_store
            .insert(vector.clone(), response.to_string())
            .await
        {
            tracing::warn!(error = %e, "Failed to insert response into vector cache");
        }
    }
}

/// Generate mock documents simulating a knowledge base retrieval.
///
/// In production, this would be replaced by an actual call to a vector
/// database or full-text search engine.
fn generate_mock_documents(count: usize) -> Vec<String> {
    (0..count)
        .map(|i| {
            format!(
                "[Doc-{:02}] This is a simulated knowledge base document (#{}) \
                 containing information relevant to the user's query. In production, \
                 these would be retrieved from a vector DB or search engine.",
                i + 1,
                i + 1,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::stubs::*;
    use crate::pipeline::traits::*;
    use std::time::Duration;

    fn test_limiter() -> AdaptiveConcurrencyLimiter {
        AdaptiveConcurrencyLimiter::new(super::super::concurrency::ConcurrencyConfig {
            min_concurrency: 4,
            max_concurrency: 256,
            target_latency: Duration::from_millis(500),
            window_size: 100,
        })
    }

    fn test_suite() -> AlgorithmSuite {
        AlgorithmSuite {
            embedder: Box::new(StubEmbedder::new(128)),
            vector_store: Box::new(StubVectorStore::new()),
            intent_classifier: Box::new(StubIntentClassifier),
            local_executor: Box::new(StubLocalExecutor),
            reranker: Box::new(StubReranker),
            external_llm: Box::new(StubExternalLlm),
        }
    }

    // ── Failing stubs for error-path testing ─────────────────────

    struct FailingEmbedder;
    #[async_trait::async_trait]
    impl Embedder for FailingEmbedder {
        async fn embed(&self, _text: &str) -> anyhow::Result<Vec<f64>> {
            Err(anyhow::anyhow!("embedding service unavailable"))
        }
        fn embedding_dimension(&self) -> usize {
            128
        }
        fn model_name(&self) -> &str {
            "failing-embedder"
        }
    }

    struct FailingClassifier;
    #[async_trait::async_trait]
    impl IntentClassifier for FailingClassifier {
        async fn classify(&self, _text: &str) -> anyhow::Result<(IntentClassification, f64)> {
            Err(anyhow::anyhow!("classifier unavailable"))
        }
        fn model_name(&self) -> &str {
            "failing-classifier"
        }
    }

    struct FailingLocalExecutor;
    #[async_trait::async_trait]
    impl LocalExecutor for FailingLocalExecutor {
        async fn execute_simple(&self, _prompt: &str) -> anyhow::Result<String> {
            Err(anyhow::anyhow!("local executor unavailable"))
        }
        fn model_name(&self) -> &str {
            "failing-executor"
        }
    }

    /// A classifier that always returns Simple, so we can test the
    /// local-executor-failure path (Step 2 → fails → falls through to Layer 3).
    struct AlwaysSimpleClassifier;
    #[async_trait::async_trait]
    impl IntentClassifier for AlwaysSimpleClassifier {
        async fn classify(&self, _text: &str) -> anyhow::Result<(IntentClassification, f64)> {
            Ok((IntentClassification::Simple, 0.95))
        }
        fn model_name(&self) -> &str {
            "always-simple"
        }
    }

    struct FailingReranker;
    #[async_trait::async_trait]
    impl Reranker for FailingReranker {
        async fn rerank(
            &self,
            _prompt: &str,
            _documents: &[String],
            _top_k: usize,
        ) -> anyhow::Result<Vec<(String, f64)>> {
            Err(anyhow::anyhow!("reranker unavailable"))
        }
        fn model_name(&self) -> &str {
            "failing-reranker"
        }
    }

    struct FailingExternalLlm;
    #[async_trait::async_trait]
    impl ExternalLlm for FailingExternalLlm {
        async fn complete(
            &self,
            _prompt: &str,
            _context_documents: &[String],
        ) -> anyhow::Result<String> {
            Err(anyhow::anyhow!("external LLM unavailable"))
        }
        fn provider_name(&self) -> &str {
            "failing-provider"
        }
        fn model_name(&self) -> &str {
            "failing-model"
        }
    }

    fn test_pipeline_cfg() -> PipelineConfig {
        PipelineConfig {
            similarity_threshold: 0.92,
            rerank_top_k: 5,
        }
    }

    // ── generate_mock_documents ──────────────────────────────────

    #[test]
    fn mock_documents_count() {
        let docs = generate_mock_documents(5);
        assert_eq!(docs.len(), 5);
    }

    #[test]
    fn mock_documents_empty() {
        let docs = generate_mock_documents(0);
        assert!(docs.is_empty());
    }

    #[test]
    fn mock_documents_content() {
        let docs = generate_mock_documents(3);
        assert!(docs[0].contains("Doc-01"));
        assert!(docs[2].contains("Doc-03"));
    }

    // ── Full pipeline paths ──────────────────────────────────────

    #[tokio::test]
    async fn pipeline_simple_task_resolves_at_layer_2() {
        let limiter = test_limiter();
        let suite = test_suite();
        let cfg = test_pipeline_cfg();

        // "hello" is classified as Simple by StubIntentClassifier.
        let response = execute_pipeline("hello".into(), &limiter, &suite, &cfg).await;

        assert_eq!(response.resolved_by_layer, 2);
        assert!(response.message.contains("StubLocalExecutor"));
        assert!(response.model.is_some());
        assert!(!response.processing_log.is_empty());
    }

    #[tokio::test]
    async fn pipeline_complex_task_resolves_at_layer_3() {
        let limiter = test_limiter();
        let suite = test_suite();
        let cfg = test_pipeline_cfg();

        // "explain quantum physics" has "explain" → Complex.
        let response = execute_pipeline(
            "explain quantum physics in detail".into(),
            &limiter,
            &suite,
            &cfg,
        )
        .await;

        assert_eq!(response.resolved_by_layer, 3);
        assert!(response.message.contains("StubExternalLlm"));
        assert!(response.model.is_some());
    }

    #[tokio::test]
    async fn pipeline_cache_hit_resolves_at_layer_1() {
        let limiter = test_limiter();
        let suite = test_suite();
        let cfg = PipelineConfig {
            similarity_threshold: 0.5, // Low threshold to ensure cache hit.
            rerank_top_k: 5,
        };

        // First request — cache miss, populates cache.
        let _ = execute_pipeline("hello".into(), &limiter, &suite, &cfg).await;

        // Second identical request — should hit the cache.
        let response = execute_pipeline("hello".into(), &limiter, &suite, &cfg).await;

        assert_eq!(response.resolved_by_layer, 1);
        assert!(!response.message.is_empty());
    }

    #[tokio::test]
    async fn pipeline_load_shedding() {
        let cfg_concurrency = super::super::concurrency::ConcurrencyConfig {
            min_concurrency: 1,
            max_concurrency: 1,
            target_latency: Duration::from_millis(500),
            window_size: 100,
        };
        let limiter = AdaptiveConcurrencyLimiter::new(cfg_concurrency);
        let suite = test_suite();
        let cfg = test_pipeline_cfg();

        // Acquire the only permit externally.
        let _permit = limiter.try_acquire().unwrap();

        // Pipeline should be rejected.
        let response = execute_pipeline("hello".into(), &limiter, &suite, &cfg).await;

        assert_eq!(response.resolved_by_layer, 0);
        assert!(response.message.contains("overloaded"));
    }

    #[tokio::test]
    async fn pipeline_processing_log_populated() {
        let limiter = test_limiter();
        let suite = test_suite();
        let cfg = test_pipeline_cfg();

        let response = execute_pipeline("hello".into(), &limiter, &suite, &cfg).await;

        assert!(!response.processing_log.is_empty());
        // Should have at least concurrency + embed/cache + classifier stages.
        assert!(response.processing_log.len() >= 2);
    }

    #[tokio::test]
    async fn pipeline_total_duration_positive() {
        let limiter = test_limiter();
        let suite = test_suite();
        let cfg = test_pipeline_cfg();

        let response = execute_pipeline("hello".into(), &limiter, &suite, &cfg).await;
        // Duration should be non-negative (could be 0 for fast execution).
        assert!(response.total_duration_ms < 10_000);
    }

    #[tokio::test]
    async fn pipeline_code_task_resolves_at_layer_3() {
        let limiter = test_limiter();
        let suite = test_suite();
        let cfg = test_pipeline_cfg();

        // "code" keyword → CodeGen → non-simple → Layer 3.
        let response = execute_pipeline("write code in Rust".into(), &limiter, &suite, &cfg).await;

        assert_eq!(response.resolved_by_layer, 3);
    }

    // ── cache_response helper ────────────────────────────────────

    #[tokio::test]
    async fn cache_response_stores_when_vector_present() {
        let suite = test_suite();
        let mut ctx = PipelineContext::new("test".into());
        ctx.request_vector = Some(vec![1.0; 128]);

        cache_response(&ctx, "cached response", &suite).await;

        assert_eq!(suite.vector_store.len().await, 1);
    }

    #[tokio::test]
    async fn cache_response_noop_when_no_vector() {
        let suite = test_suite();
        let ctx = PipelineContext::new("test".into());
        // request_vector is None.

        cache_response(&ctx, "response", &suite).await;

        assert_eq!(suite.vector_store.len().await, 0);
    }

    // ── PipelineConfig ───────────────────────────────────────────

    #[test]
    fn pipeline_config_debug() {
        let cfg = test_pipeline_cfg();
        let debug = format!("{:?}", cfg);
        assert!(debug.contains("similarity_threshold"));
        assert!(debug.contains("rerank_top_k"));
    }

    // ── Error-path tests ─────────────────────────────────────────

    #[tokio::test]
    async fn pipeline_embedding_failure_skips_cache_and_reaches_layer_2() {
        let limiter = test_limiter();
        let suite = AlgorithmSuite {
            embedder: Box::new(FailingEmbedder),
            vector_store: Box::new(StubVectorStore::new()),
            intent_classifier: Box::new(StubIntentClassifier),
            local_executor: Box::new(StubLocalExecutor),
            reranker: Box::new(StubReranker),
            external_llm: Box::new(StubExternalLlm),
        };
        let cfg = test_pipeline_cfg();

        // "hello" is simple → resolved locally even though embedding failed.
        let response = execute_pipeline("hello".into(), &limiter, &suite, &cfg).await;
        assert_eq!(response.resolved_by_layer, 2);
        // Verify the processing log mentions the embedding failure.
        let log_text = serde_json::to_string(&response.processing_log).unwrap();
        assert!(log_text.contains("Embedding failed"));
    }

    #[tokio::test]
    async fn pipeline_classification_failure_defaults_to_complex() {
        let limiter = test_limiter();
        let suite = AlgorithmSuite {
            embedder: Box::new(StubEmbedder::new(128)),
            vector_store: Box::new(StubVectorStore::new()),
            intent_classifier: Box::new(FailingClassifier),
            local_executor: Box::new(StubLocalExecutor),
            reranker: Box::new(StubReranker),
            external_llm: Box::new(StubExternalLlm),
        };
        let cfg = test_pipeline_cfg();

        // Classification fails → Unclassifiable → not Simple → Layer 3.
        let response = execute_pipeline("hello".into(), &limiter, &suite, &cfg).await;
        assert_eq!(response.resolved_by_layer, 3);
        assert!(response.message.contains("StubExternalLlm"));
    }

    #[tokio::test]
    async fn pipeline_local_executor_failure_falls_through_to_layer_3() {
        let limiter = test_limiter();
        let suite = AlgorithmSuite {
            embedder: Box::new(StubEmbedder::new(128)),
            vector_store: Box::new(StubVectorStore::new()),
            intent_classifier: Box::new(AlwaysSimpleClassifier),
            local_executor: Box::new(FailingLocalExecutor),
            reranker: Box::new(StubReranker),
            external_llm: Box::new(StubExternalLlm),
        };
        let cfg = test_pipeline_cfg();

        // Classified as Simple, but local executor fails → falls through to Layer 3.
        let response = execute_pipeline("anything".into(), &limiter, &suite, &cfg).await;
        assert_eq!(response.resolved_by_layer, 3);
        let log_text = serde_json::to_string(&response.processing_log).unwrap();
        assert!(log_text.contains("Local execution failed"));
    }

    #[tokio::test]
    async fn pipeline_reranker_failure_uses_unranked_docs() {
        let limiter = test_limiter();
        let suite = AlgorithmSuite {
            embedder: Box::new(StubEmbedder::new(128)),
            vector_store: Box::new(StubVectorStore::new()),
            intent_classifier: Box::new(StubIntentClassifier),
            local_executor: Box::new(StubLocalExecutor),
            reranker: Box::new(FailingReranker),
            external_llm: Box::new(StubExternalLlm),
        };
        let cfg = test_pipeline_cfg();

        // "explain quantum physics" → Complex → reranker fails → Layer 3 still runs.
        let response = execute_pipeline(
            "explain quantum physics in detail".into(),
            &limiter,
            &suite,
            &cfg,
        )
        .await;
        assert_eq!(response.resolved_by_layer, 3);
        let log_text = serde_json::to_string(&response.processing_log).unwrap();
        assert!(log_text.contains("Reranking failed"));
    }

    #[tokio::test]
    async fn pipeline_external_llm_failure_returns_error_message() {
        let limiter = test_limiter();
        let suite = AlgorithmSuite {
            embedder: Box::new(StubEmbedder::new(128)),
            vector_store: Box::new(StubVectorStore::new()),
            intent_classifier: Box::new(StubIntentClassifier),
            local_executor: Box::new(StubLocalExecutor),
            reranker: Box::new(StubReranker),
            external_llm: Box::new(FailingExternalLlm),
        };
        let cfg = test_pipeline_cfg();

        // Complex task → Layer 3, but external LLM fails.
        let response = execute_pipeline(
            "explain quantum physics in detail".into(),
            &limiter,
            &suite,
            &cfg,
        )
        .await;
        assert_eq!(response.resolved_by_layer, 3);
        assert!(response.message.contains("Error: External LLM unavailable"));
        let log_text = serde_json::to_string(&response.processing_log).unwrap();
        assert!(log_text.contains("LLM call failed"));
    }

    #[tokio::test]
    async fn pipeline_vector_search_error_proceeds_to_layer_2() {
        // Use a VectorStore stub that returns an error on search.
        struct FailingVectorStore;
        #[async_trait::async_trait]
        impl VectorStore for FailingVectorStore {
            async fn search(&self, _qv: &[f64], _th: f64) -> anyhow::Result<Option<(String, f64)>> {
                Err(anyhow::anyhow!("vector store unavailable"))
            }
            async fn insert(&self, _e: Vec<f64>, _r: String) -> anyhow::Result<()> {
                Ok(())
            }
            async fn len(&self) -> usize {
                0
            }
        }

        let limiter = test_limiter();
        let suite = AlgorithmSuite {
            embedder: Box::new(StubEmbedder::new(128)),
            vector_store: Box::new(FailingVectorStore),
            intent_classifier: Box::new(StubIntentClassifier),
            local_executor: Box::new(StubLocalExecutor),
            reranker: Box::new(StubReranker),
            external_llm: Box::new(StubExternalLlm),
        };
        let cfg = test_pipeline_cfg();

        // "hello" → Simple → resolved at Layer 2 (vector search error is non-critical).
        let response = execute_pipeline("hello".into(), &limiter, &suite, &cfg).await;
        assert_eq!(response.resolved_by_layer, 2);
        let log_text = serde_json::to_string(&response.processing_log).unwrap();
        assert!(log_text.contains("Search failed"));
    }
}
