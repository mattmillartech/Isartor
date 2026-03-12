// =============================================================================
// tests/unit/error_tests.rs — Unit tests for the error handling module.
//
// Covers ErrorClass, GatewayError classification, and retry integration.
// =============================================================================

use isartor::errors::{ErrorClass, GatewayError};

// ── ErrorClass ───────────────────────────────────────────────────────

#[test]
fn retryable_is_not_fatal() {
    assert_ne!(ErrorClass::Retryable, ErrorClass::Fatal);
}

#[test]
fn error_class_clone_and_copy() {
    let a = ErrorClass::Retryable;
    let b = a; // Copy
    let c = a; // Clone
    assert_eq!(a, b);
    assert_eq!(a, c);
}

// ── GatewayError classification ──────────────────────────────────────

#[test]
fn llm_timeout_is_retryable() {
    let err = anyhow::anyhow!("request timed out");
    let gw = GatewayError::from_llm_error("openai", &err);
    assert!(gw.is_retryable());
    assert_eq!(gw.class(), ErrorClass::Retryable);
}

#[test]
fn llm_401_is_fatal() {
    let err = anyhow::anyhow!("HTTP 401 Unauthorized");
    let gw = GatewayError::from_llm_error("openai", &err);
    assert!(!gw.is_retryable());
    assert_eq!(gw.class(), ErrorClass::Fatal);
}

#[test]
fn llm_403_is_fatal() {
    let err = anyhow::anyhow!("HTTP 403 Forbidden");
    let gw = GatewayError::from_llm_error("azure", &err);
    assert_eq!(gw.class(), ErrorClass::Fatal);
}

#[test]
fn llm_429_rate_limit_is_retryable() {
    let err = anyhow::anyhow!("HTTP 429 Too Many Requests");
    let gw = GatewayError::from_llm_error("openai", &err);
    assert!(gw.is_retryable());
}

#[test]
fn llm_502_bad_gateway_is_retryable() {
    let err = anyhow::anyhow!("Bad Gateway (502)");
    let gw = GatewayError::from_llm_error("anthropic", &err);
    assert!(gw.is_retryable());
}

#[test]
fn llm_503_service_unavailable_is_retryable() {
    let err = anyhow::anyhow!("503 Service Unavailable");
    let gw = GatewayError::from_llm_error("xai", &err);
    assert!(gw.is_retryable());
}

#[test]
fn llm_connection_refused_is_retryable() {
    let err = anyhow::anyhow!("connection refused");
    let gw = GatewayError::from_llm_error("openai", &err);
    assert!(gw.is_retryable());
}

#[test]
fn llm_dns_failure_is_retryable() {
    let err = anyhow::anyhow!("DNS resolution failed for api.openai.com");
    let gw = GatewayError::from_llm_error("openai", &err);
    assert!(gw.is_retryable());
}

#[test]
fn llm_invalid_api_key_is_fatal() {
    let err = anyhow::anyhow!("Error: invalid api key provided");
    let gw = GatewayError::from_llm_error("azure", &err);
    assert_eq!(gw.class(), ErrorClass::Fatal);
}

#[test]
fn llm_model_not_found_is_fatal() {
    let err = anyhow::anyhow!("model not found: gpt-5-turbo");
    let gw = GatewayError::from_llm_error("openai", &err);
    assert_eq!(gw.class(), ErrorClass::Fatal);
}

#[test]
fn unknown_error_defaults_to_retryable() {
    let err = anyhow::anyhow!("something weird happened");
    let gw = GatewayError::from_llm_error("openai", &err);
    assert!(gw.is_retryable());
}

// ── Specific variant tests ───────────────────────────────────────────

#[test]
fn validation_error_is_always_fatal() {
    let gw = GatewayError::Validation {
        message: "missing prompt".into(),
    };
    assert_eq!(gw.class(), ErrorClass::Fatal);
    assert_eq!(gw.layer_label(), "Validation");
}

#[test]
fn configuration_error_is_always_fatal() {
    let gw = GatewayError::Configuration {
        message: "no state".into(),
    };
    assert_eq!(gw.class(), ErrorClass::Fatal);
    assert_eq!(gw.layer_label(), "Configuration");
}

#[test]
fn cache_error_is_retryable() {
    let gw = GatewayError::cache_error("L1a_ExactCache", "redis timeout");
    assert!(gw.is_retryable());
    assert_eq!(gw.layer_label(), "L1a_ExactCache");
}

#[test]
fn embedding_error_is_retryable() {
    let gw = GatewayError::embedding_error("candle panicked");
    assert!(gw.is_retryable());
    assert_eq!(gw.layer_label(), "L1b_Embedding");
}

#[test]
fn inference_error_layer_label() {
    let err = anyhow::anyhow!("sidecar down");
    let gw = GatewayError::from_inference_error(&err);
    assert_eq!(gw.layer_label(), "L2_SLM");
}

// ── Display formatting ───────────────────────────────────────────────

#[test]
fn cloud_llm_display_format() {
    let gw = GatewayError::CloudLlm {
        provider: "openai".into(),
        message: "timeout".into(),
        class: ErrorClass::Retryable,
    };
    assert_eq!(format!("{gw}"), "[openai] timeout");
}

#[test]
fn cache_display_format() {
    let gw = GatewayError::cache_error("L1a_ExactCache", "connection lost");
    let display = format!("{gw}");
    assert!(display.contains("L1a_ExactCache"));
    assert!(display.contains("connection lost"));
}

#[test]
fn validation_display_format() {
    let gw = GatewayError::Validation {
        message: "bad json".into(),
    };
    let display = format!("{gw}");
    assert!(display.contains("validation"));
    assert!(display.contains("bad json"));
}
