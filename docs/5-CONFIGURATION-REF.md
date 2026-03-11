# Isartor Configuration Reference

## Master Configuration Table

| YAML Key                | Environment Variable           | Type     | Default                | Description                                      |
|------------------------ |------------------------------- |----------|------------------------|--------------------------------------------------|
| server.host             | ISARTOR__HOST                  | string   | 0.0.0.0                | Host address for server binding                  |
| server.port             | ISARTOR__PORT                  | int      | 8080                   | Port for HTTP server                             |
| exact_cache.provider    | ISARTOR__CACHE_BACKEND         | string   | memory                 | Layer 1a cache backend: memory or redis          |
| exact_cache.redis_url   | ISARTOR__REDIS_URL             | string   | (none)                 | Redis connection string (if provider=redis)      |
| exact_cache.redis_db    | ISARTOR__REDIS_DB              | int      | 0                      | Redis database index                             |
| semantic_cache.provider | ISARTOR__SEMANTIC_BACKEND      | string   | candle                 | Layer 1b semantic cache: candle (in-process) or tei (external) |
| semantic_cache.remote_url| ISARTOR__TEI_URL               | string   | (none)                 | TEI endpoint (if provider=tei)                   |
| slm_router.provider     | ISARTOR__ROUTER_BACKEND        | string   | embedded               | Layer 2 router: embedded or vllm                 |
| slm_router.remote_url   | ISARTOR__VLLM_URL              | string   | (none)                 | vLLM/TGI endpoint (if provider=vllm)             |
| slm_router.model        | ISARTOR__VLLM_MODEL            | string   | gemma-2-2b-it          | Model name/path for SLM router                   |
| slm_router.model_path   | ISARTOR__MODEL_PATH            | string   | (baked-in)             | Path to GGUF model file (embedded mode)          |
| fallback.openai_api_key | ISARTOR__OPENAI_API_KEY        | string   | (none)                 | OpenAI API key for Layer 3 fallback              |
| fallback.anthropic_api_key| ISARTOR__ANTHROPIC_API_KEY   | string   | (none)                 | Anthropic API key for Layer 3 fallback           |

## Sections

### Server
- `server.host`, `server.port`: Bind address and port.

### Layer 1a: Exact Match Cache
- `exact_cache.provider`: `memory` or `redis`
- `exact_cache.redis_url`, `exact_cache.redis_db`: Redis config

### Layer 1b: Semantic Cache
- `semantic_cache.provider`: `candle` or `tei`
- `semantic_cache.remote_url`: TEI endpoint

### Layer 2: SLM Router
- `slm_router.provider`: `embedded` or `vllm`
- `slm_router.remote_url`, `slm_router.model`, `slm_router.model_path`: Router config

### Layer 3: Cloud Fallbacks
- `fallback.openai_api_key`, `fallback.anthropic_api_key`: API keys for external LLMs

---

For full details, see [README.md](../README.md) and [docs/2-ARCHITECTURE.md](2-ARCHITECTURE.md).
