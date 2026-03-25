// ═════════════════════════════════════════════════════════════════════
// InstructionCache — Per-session dedup state for compression stages.
// ═════════════════════════════════════════════════════════════════════

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;

/// Per-session instruction hash cache for dedup across turns.
///
/// Shared via `Arc<InstructionCache>` on `AppState`.  The `DedupStage`
/// reads and writes through the `CompressionInput` reference.
#[derive(Debug, Default)]
pub struct InstructionCache {
    /// Map from session_scope → (instruction_hash, turn_count)
    seen: Mutex<HashMap<String, (String, u32)>>,
}

impl InstructionCache {
    pub fn new() -> Self {
        Self {
            seen: Mutex::new(HashMap::new()),
        }
    }

    /// Check if we've seen these exact instructions for this session before.
    /// Returns `Some(turn)` if this is a repeat (dedup candidate).
    pub fn check_and_update(&self, scope: &str, hash: &str) -> Option<u32> {
        let mut map = self.seen.lock().unwrap();
        if let Some((prev_hash, turn)) = map.get_mut(scope) {
            if prev_hash == hash {
                *turn += 1;
                return Some(*turn);
            }
            *prev_hash = hash.to_string();
            *turn = 1;
            None
        } else {
            map.insert(scope.to_string(), (hash.to_string(), 1));
            None
        }
    }

    /// Evict stale entries (called periodically or on capacity).
    pub fn evict_if_needed(&self, max_entries: usize) {
        let mut map = self.seen.lock().unwrap();
        if map.len() > max_entries {
            let keys: Vec<String> = map.keys().take(map.len() / 2).cloned().collect();
            for k in keys {
                map.remove(&k);
            }
        }
    }
}

/// SHA-256 based instruction hash for dedup comparison.
pub fn hash_instructions(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    hex::encode(&digest[..8]) // 16-char hex prefix is enough for dedup
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_cache_first_insert() {
        let cache = InstructionCache::new();
        assert!(cache.check_and_update("s1", "h1").is_none());
    }

    #[test]
    fn dedup_cache_repeat() {
        let cache = InstructionCache::new();
        cache.check_and_update("s1", "h1");
        assert_eq!(cache.check_and_update("s1", "h1"), Some(2));
        assert_eq!(cache.check_and_update("s1", "h1"), Some(3));
    }

    #[test]
    fn dedup_cache_changed_hash_resets() {
        let cache = InstructionCache::new();
        cache.check_and_update("s1", "h1");
        cache.check_and_update("s1", "h1"); // turn 2
        assert!(cache.check_and_update("s1", "h2").is_none()); // reset
        assert_eq!(cache.check_and_update("s1", "h2"), Some(2));
    }

    #[test]
    fn hash_instructions_deterministic() {
        let h1 = hash_instructions("hello");
        let h2 = hash_instructions("hello");
        assert_eq!(h1, h2);
        assert_ne!(h1, hash_instructions("world"));
    }

    #[test]
    fn evict_if_needed_reduces_size() {
        let cache = InstructionCache::new();
        for i in 0..20 {
            cache.check_and_update(&format!("s{i}"), "h");
        }
        cache.evict_if_needed(10);
        let map = cache.seen.lock().unwrap();
        assert!(map.len() <= 15); // evicted ~half of 20
    }

    // ── Hash property tests ─────────────────────────────────────

    #[test]
    fn hash_length_is_16_hex_chars() {
        let h = hash_instructions("anything");
        assert_eq!(h.len(), 16, "hash should be 16 hex chars (8 bytes)");
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn hash_empty_string() {
        let h = hash_instructions("");
        assert_eq!(h.len(), 16);
        // Should be deterministic
        assert_eq!(h, hash_instructions(""));
    }

    #[test]
    fn hash_sensitive_to_whitespace() {
        let h1 = hash_instructions("hello world");
        let h2 = hash_instructions("hello  world");
        assert_ne!(
            h1, h2,
            "Different whitespace should produce different hashes"
        );
    }

    #[test]
    fn hash_sensitive_to_case() {
        let h1 = hash_instructions("Hello");
        let h2 = hash_instructions("hello");
        assert_ne!(h1, h2);
    }

    // ── Cache concurrency ───────────────────────────────────────

    #[test]
    fn concurrent_sessions_isolated() {
        let cache = InstructionCache::new();
        cache.check_and_update("session-a", "hash-1");
        cache.check_and_update("session-b", "hash-1");

        // Repeat in session-a only
        assert_eq!(cache.check_and_update("session-a", "hash-1"), Some(2));
        // session-b is still on turn 1 repeat
        assert_eq!(cache.check_and_update("session-b", "hash-1"), Some(2));
    }

    #[test]
    fn evict_noop_under_threshold() {
        let cache = InstructionCache::new();
        for i in 0..5 {
            cache.check_and_update(&format!("s{i}"), "h");
        }
        cache.evict_if_needed(10); // 5 < 10, should be no-op
        let map = cache.seen.lock().unwrap();
        assert_eq!(map.len(), 5);
    }

    #[test]
    fn evict_at_exact_threshold() {
        let cache = InstructionCache::new();
        for i in 0..10 {
            cache.check_and_update(&format!("s{i}"), "h");
        }
        cache.evict_if_needed(10); // 10 == 10, should be no-op
        let map = cache.seen.lock().unwrap();
        assert_eq!(map.len(), 10);
    }

    #[test]
    fn multiple_evictions_converge() {
        let cache = InstructionCache::new();
        for i in 0..100 {
            cache.check_and_update(&format!("s{i}"), "h");
        }
        cache.evict_if_needed(10);
        let len1 = cache.seen.lock().unwrap().len();
        assert!(len1 < 100);

        cache.evict_if_needed(10);
        let len2 = cache.seen.lock().unwrap().len();
        assert!(len2 <= len1);
    }
}
