// =============================================================================
// RigExternalLlm — Layer 3 external LLM adapter wrapping the existing
// rig-core multi-provider Agent infrastructure.
//
// Constructs a context-augmented prompt by prepending the reranked
// context documents to the user's original prompt, then dispatches
// to the configured LLM provider (OpenAI, Azure, Anthropic, xAI).
// =============================================================================

use std::sync::Arc;

use async_trait::async_trait;

use crate::pipeline::traits::ExternalLlm;
use crate::state::AppLlmAgent;

/// Production external LLM implementation wrapping rig-core Agents.
///
/// Re-uses the existing `AppLlmAgent` trait from `state.rs`, which
/// already supports OpenAI, Azure, Anthropic, and xAI providers.
pub struct RigExternalLlm {
    /// The underlying rig-core agent (shared with the v1 handler).
    agent: Arc<dyn AppLlmAgent>,

    /// Model name for observability (e.g. "gpt-4o-mini").
    model: String,
}

impl RigExternalLlm {
    pub fn new(agent: Arc<dyn AppLlmAgent>, model: String) -> Self {
        Self { agent, model }
    }
}

#[async_trait]
impl ExternalLlm for RigExternalLlm {
    async fn complete(&self, prompt: &str, context_documents: &[String]) -> anyhow::Result<String> {
        // Construct the context-augmented prompt.
        let augmented_prompt = if context_documents.is_empty() {
            prompt.to_string()
        } else {
            let context_block = context_documents
                .iter()
                .enumerate()
                .map(|(i, doc)| format!("[Context Document {}]\n{}", i + 1, doc))
                .collect::<Vec<_>>()
                .join("\n\n");

            format!(
                "Use the following context documents to inform your response. \
                 If the context is not relevant, answer based on your own knowledge.\n\n\
                 ---BEGIN CONTEXT---\n\
                 {context_block}\n\
                 ---END CONTEXT---\n\n\
                 User question: {prompt}"
            )
        };

        tracing::debug!(
            prompt_len = augmented_prompt.len(),
            context_docs = context_documents.len(),
            provider = self.agent.provider_name(),
            model = %self.model,
            "RigExternalLlm: dispatching augmented prompt"
        );

        let response = self.agent.chat(&augmented_prompt).await?;

        tracing::debug!(
            response_len = response.len(),
            "RigExternalLlm: completion received"
        );

        Ok(response)
    }

    fn provider_name(&self) -> &str {
        self.agent.provider_name()
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A mock LLM agent for testing RigExternalLlm.
    struct MockAgent {
        provider: &'static str,
        response: Result<String, String>,
    }

    #[async_trait]
    impl AppLlmAgent for MockAgent {
        async fn chat(&self, _prompt: &str) -> anyhow::Result<String> {
            match &self.response {
                Ok(s) => Ok(s.clone()),
                Err(e) => Err(anyhow::anyhow!(e.clone())),
            }
        }
        fn provider_name(&self) -> &'static str {
            self.provider
        }
    }

    /// A mock agent that captures the prompt it receives.
    struct CapturingAgent {
        captured: tokio::sync::Mutex<Option<String>>,
    }

    #[async_trait]
    impl AppLlmAgent for CapturingAgent {
        async fn chat(&self, prompt: &str) -> anyhow::Result<String> {
            *self.captured.lock().await = Some(prompt.to_string());
            Ok("ok".to_string())
        }
        fn provider_name(&self) -> &'static str {
            "capturing"
        }
    }

    #[tokio::test]
    async fn complete_no_context_documents() {
        let agent = Arc::new(CapturingAgent {
            captured: tokio::sync::Mutex::new(None),
        });
        let llm = RigExternalLlm::new(agent.clone(), "test-model".into());

        let docs: Vec<String> = vec![];
        let result = llm.complete("hello world", &docs).await.unwrap();
        assert_eq!(result, "ok");

        // With empty docs, the prompt should be passed through unchanged.
        let captured = agent.captured.lock().await;
        assert_eq!(captured.as_deref(), Some("hello world"));
    }

    #[tokio::test]
    async fn complete_with_context_documents() {
        let agent = Arc::new(CapturingAgent {
            captured: tokio::sync::Mutex::new(None),
        });
        let llm = RigExternalLlm::new(agent.clone(), "gpt-4o".into());

        let docs = vec![
            "Document A content".to_string(),
            "Document B content".to_string(),
        ];
        let _result = llm.complete("What is Rust?", &docs).await.unwrap();

        let captured = agent.captured.lock().await;
        let prompt = captured.as_ref().unwrap();

        // Should contain context block markers and both documents.
        assert!(prompt.contains("---BEGIN CONTEXT---"));
        assert!(prompt.contains("---END CONTEXT---"));
        assert!(prompt.contains("[Context Document 1]"));
        assert!(prompt.contains("Document A content"));
        assert!(prompt.contains("[Context Document 2]"));
        assert!(prompt.contains("Document B content"));
        assert!(prompt.contains("What is Rust?"));
    }

    #[tokio::test]
    async fn complete_agent_error_propagates() {
        let agent = Arc::new(MockAgent {
            provider: "openai",
            response: Err("LLM overloaded".into()),
        });
        let llm = RigExternalLlm::new(agent, "gpt-4o".into());

        let docs: Vec<String> = vec![];
        let result = llm.complete("test", &docs).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("LLM overloaded"));
    }

    #[tokio::test]
    async fn provider_name_delegates_to_agent() {
        let agent = Arc::new(MockAgent {
            provider: "anthropic",
            response: Ok("fine".into()),
        });
        let llm = RigExternalLlm::new(agent, "claude-3".into());
        assert_eq!(llm.provider_name(), "anthropic");
    }

    #[tokio::test]
    async fn model_name_returns_configured_value() {
        let agent = Arc::new(MockAgent {
            provider: "xai",
            response: Ok("fine".into()),
        });
        let llm = RigExternalLlm::new(agent, "grok-beta".into());
        assert_eq!(llm.model_name(), "grok-beta");
    }
}
