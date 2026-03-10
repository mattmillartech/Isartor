// =============================================================================
// Services Module — Embedded ML inference services.
//
// Houses Rust-native model inference using the candle framework,
// eliminating the need for external sidecar processes.
// =============================================================================

#[cfg(feature = "embedded-inference")]
pub mod local_inference;
