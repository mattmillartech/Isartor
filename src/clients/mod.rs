// =============================================================================
// Client Modules — Dedicated HTTP clients for sidecar services.
//
// Each sidecar gets its own client with:
//   - Pre-configured timeouts from `Layer2Settings` / `EmbeddingSidecarSettings`
//   - Type-safe request/response serialization
//   - Structured error handling
// =============================================================================

pub mod slm;
