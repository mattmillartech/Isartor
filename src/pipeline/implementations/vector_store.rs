// =============================================================================
// InMemoryVectorStore — Layer 1 VectorStore with TTL, capacity management,
// and brute-force cosine-similarity search.
//
// This is a production-grade in-memory store. For datasets > ~100k entries,
// swap in an external vector DB (Qdrant, Weaviate) or ANN library.
// The interface stays identical — only the search algorithm changes.
// =============================================================================

use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::pipeline::traits::VectorStore;

// ── Entry ────────────────────────────────────────────────────────────

struct CacheEntry {
    embedding: Vec<f64>,
    response: String,
    created_at: Instant,
}

// ── Store ────────────────────────────────────────────────────────────

/// In-memory vector cache with TTL expiry and capacity eviction.
///
/// Search is brute-force cosine similarity (O(n)). For the expected
/// cache sizes (< 50k entries) this completes in < 2ms on modern
/// hardware. For larger stores, the `VectorStore` trait can be backed
/// by an HNSW index without changing the pipeline.
pub struct InMemoryVectorStore {
    entries: RwLock<Vec<CacheEntry>>,
    ttl: Duration,
    max_capacity: usize,
}

impl InMemoryVectorStore {
    pub fn new(ttl_secs: u64, max_capacity: u64) -> Self {
        Self {
            entries: RwLock::new(Vec::new()),
            ttl: Duration::from_secs(ttl_secs),
            max_capacity: max_capacity as usize,
        }
    }
}

#[async_trait]
impl VectorStore for InMemoryVectorStore {
    async fn search(
        &self,
        query_vector: &[f64],
        threshold: f64,
    ) -> anyhow::Result<Option<(String, f64)>> {
        let entries = self.entries.read().await;
        let now = Instant::now();

        let mut best_score: f64 = 0.0;
        let mut best_response: Option<&str> = None;

        for entry in entries.iter() {
            // Lazy TTL expiry — skip stale entries during search.
            if now.duration_since(entry.created_at) > self.ttl {
                continue;
            }
            let score = cosine_similarity(query_vector, &entry.embedding);
            if score >= threshold && score > best_score {
                best_score = score;
                best_response = Some(&entry.response);
            }
        }

        if let Some(resp) = best_response {
            tracing::debug!(
                similarity = format!("{best_score:.4}"),
                threshold = format!("{threshold:.4}"),
                "InMemoryVectorStore: cache HIT"
            );
            Ok(Some((resp.to_string(), best_score)))
        } else {
            Ok(None)
        }
    }

    async fn insert(&self, embedding: Vec<f64>, response: String) -> anyhow::Result<()> {
        let mut entries = self.entries.write().await;
        let now = Instant::now();

        // Evict expired entries.
        entries.retain(|e| now.duration_since(e.created_at) <= self.ttl);

        // If still at capacity, remove the oldest entry (FIFO eviction).
        while entries.len() >= self.max_capacity {
            entries.remove(0);
        }

        entries.push(CacheEntry {
            embedding,
            response,
            created_at: now,
        });

        tracing::debug!(size = entries.len(), "InMemoryVectorStore: inserted entry");
        Ok(())
    }

    async fn len(&self) -> usize {
        self.entries.read().await.len()
    }
}

// ── Cosine similarity ────────────────────────────────────────────────

/// Compute cosine similarity between two f64 vectors.
///
/// Returns a value in [-1, 1]. Returns 0.0 for degenerate inputs.
fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot: f64 = 0.0;
    let mut mag_a: f64 = 0.0;
    let mut mag_b: f64 = 0.0;

    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        mag_a += x * x;
        mag_b += y * y;
    }

    let denom = mag_a.sqrt() * mag_b.sqrt();
    if denom == 0.0 { 0.0 } else { dot / denom }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::traits::VectorStore;

    #[tokio::test]
    async fn insert_and_search_hit() {
        let store = InMemoryVectorStore::new(300, 100);
        let embedding = vec![1.0, 0.0, 0.0];
        store
            .insert(embedding.clone(), "response".into())
            .await
            .unwrap();

        let result = store.search(&embedding, 0.9).await.unwrap();
        assert!(result.is_some());
        let (resp, score) = result.unwrap();
        assert_eq!(resp, "response");
        assert!((score - 1.0).abs() < 1e-6);
    }

    #[tokio::test]
    async fn search_miss_below_threshold() {
        let store = InMemoryVectorStore::new(300, 100);
        store.insert(vec![1.0, 0.0, 0.0], "a".into()).await.unwrap();
        let query = vec![0.0, 1.0, 0.0]; // orthogonal
        let result = store.search(&query, 0.5).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn search_empty_store() {
        let store = InMemoryVectorStore::new(300, 100);
        let query = vec![1.0, 0.0];
        let result = store.search(&query, 0.5).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn ttl_expiry() {
        let store = InMemoryVectorStore::new(0, 100); // TTL = 0s
        let emb = vec![1.0, 0.0];
        store.insert(emb.clone(), "cached".into()).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let result = store.search(&emb, 0.5).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn capacity_eviction() {
        let store = InMemoryVectorStore::new(300, 2);
        store.insert(vec![1.0, 0.0], "first".into()).await.unwrap();
        store.insert(vec![0.0, 1.0], "second".into()).await.unwrap();
        store.insert(vec![0.5, 0.5], "third".into()).await.unwrap();
        assert_eq!(store.len().await, 2);
    }

    #[tokio::test]
    async fn len_tracking() {
        let store = InMemoryVectorStore::new(300, 100);
        assert_eq!(store.len().await, 0);
        store.insert(vec![1.0], "a".into()).await.unwrap();
        assert_eq!(store.len().await, 1);
        store.insert(vec![2.0], "b".into()).await.unwrap();
        assert_eq!(store.len().await, 2);
    }

    #[tokio::test]
    async fn returns_best_match() {
        let store = InMemoryVectorStore::new(300, 100);
        store.insert(vec![1.0, 0.1], "close".into()).await.unwrap();
        store.insert(vec![1.0, 0.0], "exact".into()).await.unwrap();
        let query = vec![1.0, 0.0];
        let result = store.search(&query, 0.5).await.unwrap();
        assert!(result.is_some());
        let (resp, _) = result.unwrap();
        assert_eq!(resp, "exact");
    }

    // ── cosine_similarity unit tests ─────────────────────────────

    #[test]
    fn cosine_identical() {
        let v = vec![1.0, 2.0, 3.0];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn cosine_orthogonal() {
        assert!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-9);
    }

    #[test]
    fn cosine_empty() {
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
    }

    #[test]
    fn cosine_mismatched() {
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 2.0]), 0.0);
    }

    #[test]
    fn cosine_zero_vector() {
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 2.0]), 0.0);
    }
}
