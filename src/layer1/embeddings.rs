use anyhow::{Context, Result};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::sync::Mutex;

/// A struct that holds the loaded embedding model state so it persists across requests.
/// The inner Mutex is required because fastembed's `embed()` takes `&mut self`.
/// We use `std::sync::Mutex` (not tokio) because inference is CPU-bound and
/// always called from `spawn_blocking`.
pub struct TextEmbedder {
    model: Mutex<TextEmbedding>,
}

impl TextEmbedder {
    /// Initializes the fastembed::TextEmbedding model.
    /// Uses BAAI/bge-small-en-v1.5, optimized for sentence similarity.
    /// This function blocks during startup while downloading and loading the model into RAM.
    pub fn new() -> Result<Self> {
        let model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::BGESmallENV15).with_show_download_progress(true),
        )
        .context("Failed to initialize fastembed TextEmbedding model: BGE-Small-EN-v1.5")?;

        Ok(Self {
            model: Mutex::new(model),
        })
    }

    /// Generates embeddings for a single prompt string.
    pub fn generate_embedding(&self, text: &str) -> Result<Vec<f32>> {
        let mut model = self
            .model
            .lock()
            .map_err(|e| anyhow::anyhow!("embedder lock poisoned: {e}"))?;
        // fastembed expects a batch, so we wrap our single text in a Vec
        let mut embeddings = model
            .embed(vec![text], None)
            .context("Failed to generate embedding for the provided text")?;

        // Extract the first (and only) vector from the result batch
        let embedding = embeddings
            .pop()
            .context("Embedding result returned empty batch")?;

        Ok(embedding)
    }
}

/// Helper function to calculate the cosine similarity between two slice references.
/// Formula: (A dot B) / (||A|| * ||B||)
#[allow(dead_code)]
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let mut dot_product = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;

    for (val_a, val_b) in a.iter().zip(b.iter()) {
        dot_product += val_a * val_b;
        norm_a += val_a * val_a;
        norm_b += val_b * val_b;
    }

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot_product / (norm_a.sqrt() * norm_b.sqrt())
}

/// Returns a shared singleton `TextEmbedder` for use in tests.
/// Avoids fastembed file-lock contention when many tests run in parallel.
#[cfg(test)]
pub fn shared_test_embedder() -> std::sync::Arc<TextEmbedder> {
    use std::sync::{Arc, LazyLock};
    static EMBEDDER: LazyLock<Arc<TextEmbedder>> =
        LazyLock::new(|| Arc::new(TextEmbedder::new().expect("shared test TextEmbedder")));
    EMBEDDER.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_embedder_flow() {
        // Use the shared singleton to avoid lock contention
        let embedder = shared_test_embedder();

        let text1 = "Where is the pricing page?";
        let text2 = "How much does it cost?";
        let text3 = "I like eating apples.";

        // Generate embeddings
        let emb1 = embedder
            .generate_embedding(text1)
            .expect("Failed embedding 1");
        let emb2 = embedder
            .generate_embedding(text2)
            .expect("Failed embedding 2");
        let emb3 = embedder
            .generate_embedding(text3)
            .expect("Failed embedding 3");

        // Calculate similarities
        let sim1_2 = cosine_similarity(&emb1, &emb2);
        let sim1_3 = cosine_similarity(&emb1, &emb3);

        println!("Similarity between semantic matches: {}", sim1_2);
        println!("Similarity between unrelated: {}", sim1_3);

        // Assert that the semantic match is reasonably high (BGE-small-en-v1.5 yields ~0.69 here)
        assert!(
            sim1_2 > 0.6,
            "Similarity should be high for semantic matches, got {sim1_2}"
        );
        // Assert that the unrelated text has lower similarity
        assert!(
            sim1_2 > sim1_3,
            "Semantic match should score higher than unrelated"
        );
    }

    #[test]
    fn test_cosine_similarity_edge_cases() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0); // Orthogonal

        let a = vec![1.0, 2.0, 3.0];
        assert!((cosine_similarity(&a, &a) - 1.0).abs() < f32::EPSILON); // Identical

        let zero = vec![0.0, 0.0];
        let b = vec![1.0, 1.0];
        assert_eq!(cosine_similarity(&zero, &b), 0.0); // Zero magnitude
    }
}
