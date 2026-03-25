// ═════════════════════════════════════════════════════════════════════
// L2.5 Context Compression — Re-export shim
// ═════════════════════════════════════════════════════════════════════
//
// All compression logic has been moved to `src/compression/`.
// This module re-exports the public API for backward compatibility
// so existing `use crate::core::context_compress::...` imports
// continue to work.

pub use crate::compression::cache::{InstructionCache, hash_instructions};
pub use crate::compression::optimize::{OptimizeResult, build_pipeline, optimize_request_body};
