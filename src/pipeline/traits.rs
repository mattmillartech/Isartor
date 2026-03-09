// =============================================================================
// Algorithmic Interface Definitions — The trait contracts that every ML
// algorithm implementation must satisfy.
//
// These traits define the extension points of the pipeline. Each one
// corresponds to a specific algorithmic capability that will be plugged
// in with a concrete implementation (ONNX runtime, HNSW index, Cross-
// Encoder model, etc.).
//
// The traits use `async_trait` because every real implementation will
// involve I/O (model inference, vector DB queries, HTTP calls).
// =============================================================================

use async_trait::async_trait;

// ═════════════════════════════════════════════════════════════════════
// Layer 1 — Semantic Cache
// ═════════════════════════════════════════════════════════════════════

/// Converts raw text into a dense embedding vector.
///
/// The embedding is used for:
/// - Semantic cache lookup (Layer 1)
/// - Similarity-based routing decisions
///
/// # TODO: Implement using local ONNX runtime (e.g., Sentence-BERT).
///
/// The production implementation should:
/// 1. Load a pre-trained Sentence-BERT (or similar) ONNX model at startup.
/// 2. Tokenise the input text using the model's tokeniser.
/// 3. Run inference on the ONNX runtime to produce a fixed-dimension vector.
/// 4. Normalise the vector to unit length for cosine similarity.
///
/// Expected latency budget: < 5ms for local ONNX inference.
#[async_trait]
pub trait Embedder: Send + Sync {
    /// Embed the given text into a dense vector representation.
    ///
    /// Returns a vector of f64 values. The dimensionality depends on the
    /// underlying model (e.g., 384 for MiniLM, 768 for BERT-base).
    async fn embed(&self, text: &str) -> anyhow::Result<Vec<f64>>;

    /// Returns the dimensionality of vectors produced by this embedder.
    #[allow(dead_code)]
    fn embedding_dimension(&self) -> usize;

    /// Returns the model identifier (for observability / logging).
    fn model_name(&self) -> &str;
}

/// Approximate nearest-neighbour search over cached embeddings.
///
/// Stores prompt embeddings keyed to their cached responses, enabling
/// semantic deduplication of requests.
///
/// # TODO: Implement using HNSW index for approximate nearest neighbor search.
///
/// The production implementation should:
/// 1. Use an HNSW (Hierarchical Navigable Small World) graph index
///    (e.g., via the `hnsw_rs` or `instant-distance` crate, or Qdrant).
/// 2. Support configurable `ef_construction` and `M` parameters for
///    index build quality vs. speed trade-off.
/// 3. Return the closest vector + its associated response if the cosine
///    similarity exceeds the threshold.
/// 4. Handle index persistence (snapshot / restore) for crash recovery.
///
/// Expected latency budget: < 1ms for in-memory HNSW search.
#[async_trait]
pub trait VectorStore: Send + Sync {
    /// Search for the nearest cached response to the given query vector.
    ///
    /// Returns `Some((response, similarity_score))` if a match is found
    /// above the given `threshold`, or `None` if no sufficiently similar
    /// entry exists.
    async fn search(
        &self,
        query_vector: &[f64],
        threshold: f64,
    ) -> anyhow::Result<Option<(String, f64)>>;

    /// Insert a new embedding → response pair into the store.
    async fn insert(&self, embedding: Vec<f64>, response: String) -> anyhow::Result<()>;

    /// Returns the current number of entries in the store.
    async fn len(&self) -> usize;

    /// Returns `true` if the store contains no entries.
    #[allow(dead_code)]
    async fn is_empty(&self) -> bool {
        self.len().await == 0
    }
}

// ═════════════════════════════════════════════════════════════════════
// Layer 2 — SLM Router
// ═════════════════════════════════════════════════════════════════════

/// Classifies the intent and complexity of an incoming prompt.
///
/// The classification result determines the pipeline routing:
/// - `Simple` → handled by the local SLM executor (short-circuit).
/// - `Complex` / `Rag` / `CodeGen` → proceeds to Layer 2.5 and Layer 3.
///
/// # TODO: Implement using local SLM performing Zero-Shot NLI task.
///
/// The production implementation should:
/// 1. Use a local Small Language Model (e.g., Phi-3, TinyLlama) via
///    ONNX runtime or Ollama.
/// 2. Frame the classification as a Zero-Shot Natural Language Inference
///    (NLI) task with candidate labels ["simple", "complex", "rag", "codegen"].
/// 3. Return the top label and its softmax confidence score.
/// 4. Fall back to `Unclassifiable` if inference fails or confidence
///    is below a minimum viable threshold.
///
/// Expected latency budget: < 50ms for local SLM inference.
#[async_trait]
pub trait IntentClassifier: Send + Sync {
    /// Classify the given text into an intent label with a confidence score.
    ///
    /// Returns `(IntentLabel, ConfidenceScore)` where the score is in [0.0, 1.0].
    async fn classify(
        &self,
        text: &str,
    ) -> anyhow::Result<(super::context::IntentClassification, f64)>;

    /// Returns the model identifier (for observability / logging).
    fn model_name(&self) -> &str;
}

/// Executes simple tasks locally using an on-premise SLM.
///
/// This executor is called when the intent classifier determines the
/// task is simple enough to handle without the expensive external LLM.
///
/// The implementation may use:
/// - Ollama with a lightweight model (e.g., llama3, phi-3)
/// - A local ONNX-based text generation model
/// - A template-based response system for known simple patterns
#[async_trait]
pub trait LocalExecutor: Send + Sync {
    /// Execute a simple task and return the response text.
    async fn execute_simple(&self, prompt: &str) -> anyhow::Result<String>;

    /// Returns the model identifier (for observability / logging).
    fn model_name(&self) -> &str;
}

// ═════════════════════════════════════════════════════════════════════
// Layer 2.5 — Context Optimiser
// ═════════════════════════════════════════════════════════════════════

/// Reranks a list of candidate documents by relevance to the prompt.
///
/// After retrieving candidate documents from the knowledge base, the
/// reranker scores each document's relevance and returns only the top-K
/// most useful ones. This dramatically reduces token consumption in the
/// final LLM call.
///
/// # TODO: Implement using local Cross-Encoder model to filter irrelevant
/// #       context to save tokens.
///
/// The production implementation should:
/// 1. Use a Cross-Encoder model (e.g., `cross-encoder/ms-marco-MiniLM-L-6-v2`)
///    loaded via ONNX runtime.
/// 2. For each (prompt, document) pair, compute a relevance score.
/// 3. Sort documents by descending relevance score.
/// 4. Return the top-K documents (configurable, default K=5).
/// 5. Include relevance scores in the result for observability.
///
/// Expected latency budget: < 20ms for reranking 20 documents locally.
#[async_trait]
pub trait Reranker: Send + Sync {
    /// Rerank the given documents by relevance to the prompt.
    ///
    /// Returns the top-K documents sorted by descending relevance,
    /// paired with their relevance scores.
    async fn rerank(
        &self,
        prompt: &str,
        documents: &[String],
        top_k: usize,
    ) -> anyhow::Result<Vec<(String, f64)>>;

    /// Returns the model identifier (for observability / logging).
    fn model_name(&self) -> &str;
}

// ═════════════════════════════════════════════════════════════════════
// Layer 3 — External LLM Fallback
// ═════════════════════════════════════════════════════════════════════

/// Sends the final, context-augmented prompt to an external LLM.
///
/// This trait abstracts the external LLM provider (OpenAI, Anthropic,
/// Azure, xAI, etc.) behind a uniform interface. The pipeline passes
/// the original prompt along with the optimised context documents.
#[async_trait]
pub trait ExternalLlm: Send + Sync {
    /// Send a completion request to the external LLM.
    ///
    /// `prompt` is the original user prompt.
    /// `context_documents` is the reranked set of relevant documents
    /// that should be included as context in the LLM call.
    async fn complete(&self, prompt: &str, context_documents: &[String]) -> anyhow::Result<String>;

    /// Returns the provider name (for observability / logging).
    fn provider_name(&self) -> &str;

    /// Returns the model identifier (for observability / logging).
    fn model_name(&self) -> &str;
}

// ═════════════════════════════════════════════════════════════════════
// Composite trait alias — All algorithms bundled for DI
// ═════════════════════════════════════════════════════════════════════

/// Container holding all algorithmic components required by the pipeline.
///
/// This struct is the single injection point for the orchestrator.
/// Swap out any field to change the algorithm used at that layer.
pub struct AlgorithmSuite {
    /// Layer 1: Text → Vector embedding.
    pub embedder: Box<dyn Embedder>,

    /// Layer 1: Approximate nearest-neighbour cache.
    pub vector_store: Box<dyn VectorStore>,

    /// Layer 2: Intent / complexity classification.
    pub intent_classifier: Box<dyn IntentClassifier>,

    /// Layer 2: Local SLM for simple tasks.
    pub local_executor: Box<dyn LocalExecutor>,

    /// Layer 2.5: Cross-encoder document reranker.
    pub reranker: Box<dyn Reranker>,

    /// Layer 3: External LLM fallback.
    pub external_llm: Box<dyn ExternalLlm>,
}
