#![allow(dead_code)]
//! # Cache Adapters — Concrete implementations of `ExactCache`
//!
//! | Adapter            | Backing Store         | Use Case                |
//! |--------------------|-----------------------|-------------------------|
//! | `InMemoryCache`    | ahash + LRU (parking_lot) | Minimalist / single-node |
//! | `RedisExactCache`  | Redis (via `redis` crate) | Enterprise / K8s        |

use std::num::NonZeroUsize;
use std::sync::Arc;

use ahash::RandomState;
use async_trait::async_trait;
use lru::LruCache;
use parking_lot::RwLock;

use crate::core::ports::ExactCache;
use redis::AsyncCommands;
use tracing::Instrument;

// ═══════════════════════════════════════════════════════════════════════
// Adapter: InMemoryCache — bounded LRU with ahash + parking_lot
// ═══════════════════════════════════════════════════════════════════════

/// High-performance, concurrent, bounded LRU exact-match cache.
///
/// Uses `ahash` for fast hashing and `parking_lot::RwLock` for
/// low-overhead synchronisation. Designed for single-binary / edge
/// deployments where sub-microsecond cache lookups are critical.
pub struct InMemoryCache {
    inner: Arc<RwLock<LruCache<String, String, RandomState>>>,
}

impl InMemoryCache {
    /// Create a new in-memory LRU cache with the given maximum capacity.
    pub fn new(capacity: NonZeroUsize) -> Self {
        let cache = LruCache::with_hasher(capacity, RandomState::new());
        Self {
            inner: Arc::new(RwLock::new(cache)),
        }
    }
}

#[async_trait]
impl ExactCache for InMemoryCache {
    async fn get(&self, key: &str) -> anyhow::Result<Option<String>> {
        let span = tracing::info_span!("l1a_exact_cache_get", cache.backend = "memory", cache.key = %key, cache.hit = tracing::field::Empty);
        let _guard = span.enter();
        // LruCache::get requires &mut self to promote the entry (LRU touch).
        let mut cache = self.inner.write();
        let result = cache.get(key).cloned();
        let hit = result.is_some();
        span.record("cache.hit", hit);
        if hit {
            tracing::info!(cache.hit = true, "L1a exact cache HIT");
        } else {
            tracing::debug!(cache.hit = false, "L1a exact cache MISS");
        }
        Ok(result)
    }

    async fn put(&self, key: &str, response: &str) -> anyhow::Result<()> {
        let span = tracing::debug_span!("l1a_exact_cache_put", cache.backend = "memory", cache.key = %key, response_len = response.len());
        let _guard = span.enter();
        let mut cache = self.inner.write();
        cache.put(key.to_owned(), response.to_owned());
        tracing::debug!("L1a exact cache: stored entry");
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Adapter: RedisExactCache — distributed cache backed by Redis
// ═══════════════════════════════════════════════════════════════════════

/// Distributed exact-match cache backed by Redis.
///
/// Designed for enterprise / Kubernetes deployments where multiple Isartor
/// replicas share the same cache layer.  Uses the `redis` crate with an
/// async connection pool.
///
/// **Note:** Deep Redis logic (pipelining, Cluster support, etc.) is
/// deferred.  This skeleton demonstrates the adapter shape.
pub struct RedisExactCache {
    /// Redis connection URL (e.g. `redis://redis-master:6379`).
    url: String,
    // In a full implementation this would hold:
    // pool: redis::aio::ConnectionManager,
}

impl RedisExactCache {
    /// Create a new Redis-backed cache adapter.
    ///
    /// # Arguments
    /// * `url` — Redis connection string (e.g. `redis://redis-master:6379`).
    pub fn new(url: impl Into<String>) -> Self {
        let url = url.into();
        log::info!("RedisExactCache adapter created url={}", url);
        Self { url }
    }
}

#[async_trait]
impl ExactCache for RedisExactCache {
    async fn get(&self, key: &str) -> anyhow::Result<Option<String>> {
        let span = tracing::info_span!("l1a_exact_cache_get", cache.backend = "redis", cache.key = %key, cache.hit = tracing::field::Empty);
        async {
            tracing::debug!("RedisExactCache: GET");
            let client = redis::Client::open(self.url.as_str())?;
            let mut conn = client.get_multiplexed_tokio_connection().await?;
            let val: Option<String> = conn.get(key).await?;
            let hit = val.is_some();
            tracing::Span::current().record("cache.hit", hit);
            if hit {
                tracing::info!(cache.hit = true, "L1a Redis cache HIT");
            } else {
                tracing::debug!(cache.hit = false, "L1a Redis cache MISS");
            }
            Ok(val)
        }
        .instrument(span)
        .await
    }

    async fn put(&self, key: &str, response: &str) -> anyhow::Result<()> {
        let span = tracing::debug_span!("l1a_exact_cache_put", cache.backend = "redis", cache.key = %key, response_len = response.len());
        async {
            tracing::debug!("RedisExactCache: SET");
            let client = redis::Client::open(self.url.as_str())?;
            let mut conn = client.get_multiplexed_tokio_connection().await?;
            // Set with a default TTL (e.g., 1 hour = 3600s). Adjust as needed.
            let _: () = conn.set_ex(key, response, 3600).await?;
            tracing::debug!("L1a Redis cache: stored entry");
            Ok(())
        }
        .instrument(span)
        .await
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroUsize;

    #[tokio::test]
    async fn in_memory_cache_round_trip() {
        let cache = InMemoryCache::new(NonZeroUsize::new(64).unwrap());
        assert!(cache.get("k1").await.unwrap().is_none());
        cache.put("k1", "v1").await.unwrap();
        assert_eq!(cache.get("k1").await.unwrap(), Some("v1".into()));
    }

    #[tokio::test]
    async fn in_memory_cache_eviction() {
        let cache = InMemoryCache::new(NonZeroUsize::new(2).unwrap());
        cache.put("a", "1").await.unwrap();
        cache.put("b", "2").await.unwrap();
        // Touch "a" so it becomes most-recently used.
        let _ = cache.get("a").await.unwrap();
        // Insert "c" — should evict "b".
        cache.put("c", "3").await.unwrap();
        assert_eq!(cache.get("a").await.unwrap(), Some("1".into()));
        assert!(cache.get("b").await.unwrap().is_none());
        assert_eq!(cache.get("c").await.unwrap(), Some("3".into()));
    }

    #[tokio::test]
    #[ignore = "requires a running Redis instance on localhost:6379"]
    async fn redis_cache_skeleton_returns_none() {
        let cache = RedisExactCache::new("redis://localhost:6379");
        // Skeleton implementation always returns None.
        assert!(cache.get("any-key").await.unwrap().is_none());
        // put should not error.
        cache.put("any-key", "value").await.unwrap();
    }
}
