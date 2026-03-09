// =============================================================================
// Production Implementations — Real algorithmic components backed by
// Ollama (local SLM/embedding), in-memory HNSW-style vector store,
// and rig-core (external LLM providers).
// =============================================================================

pub mod embedder;
pub mod external_llm;
pub mod intent_classifier;
pub mod local_executor;
pub mod reranker;
pub mod vector_store;
