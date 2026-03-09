# 📄 Configuration Reference

> **Complete reference for all `ISARTOR_*` environment variables, config file support, and per-tier defaults.**

---

## Configuration Loading Order

Isartor uses the [`config`](https://crates.io/crates/config) crate with the following precedence (highest wins):

1. **Environment variables** — prefixed with `ISARTOR_`, double-underscore `__` maps to nested structs.
2. **Config file** — `isartor.toml`, `isartor.yaml`, or `isartor.json` in the working directory (optional).
3. **Compiled defaults** — sensible values baked into the binary.

```bash
# Example: nested struct via env var
# AppConfig.layer2.sidecar_url → ISARTOR_LAYER2__SIDECAR_URL
export ISARTOR_LAYER2__SIDECAR_URL="http://127.0.0.1:8081"
```

---

## Full Variable Reference

### General

| Variable | Type | Default | Description |
| --- | --- | --- | --- |
| `ISARTOR_HOST_PORT` | `String` | `0.0.0.0:8080` | Socket address the server binds to |
| `ISARTOR_GATEWAY_API_KEY` | `String` | `changeme` | API key required in `X-API-Key` header (Layer 0) |

### Layer 1 — Cache

| Variable | Type | Default | Description |
| --- | --- | --- | --- |
| `ISARTOR_CACHE_MODE` | `String` | `both` | `exact` — SHA-256 hash only; `semantic` — cosine similarity; `both` — exact first, then semantic |
| `ISARTOR_EMBEDDING_MODEL` | `String` | `all-minilm` | Embedding model name (informational, must match sidecar) |
| `ISARTOR_SIMILARITY_THRESHOLD` | `f64` | `0.85` | Cosine similarity threshold for semantic cache hits (0.0–1.0) |
| `ISARTOR_CACHE_TTL_SECS` | `u64` | `300` | Time-to-live for cached responses, in seconds |
| `ISARTOR_CACHE_MAX_CAPACITY` | `u64` | `10000` | Maximum entries per cache (exact + semantic counted separately) |

### Layer 2 — Generation Sidecar (llama.cpp)

| Variable | Type | Default | Description |
| --- | --- | --- | --- |
| `ISARTOR_LAYER2__SIDECAR_URL` | `String` | `http://127.0.0.1:8081` | Base URL of the generation sidecar (OpenAI-compatible API) |
| `ISARTOR_LAYER2__MODEL_NAME` | `String` | `phi-3-mini` | Model name sent in the API `model` field |
| `ISARTOR_LAYER2__TIMEOUT_SECONDS` | `u64` | `30` | HTTP request timeout for sidecar calls |

### Layer 2 — Legacy (v1 Middleware, Ollama-Compatible)

| Variable | Type | Default | Description |
| --- | --- | --- | --- |
| `ISARTOR_LOCAL_SLM_URL` | `String` | `http://localhost:11434/api/generate` | URL of the local SLM for v1 middleware triage |
| `ISARTOR_LOCAL_SLM_MODEL` | `String` | `llama3` | Model name for v1 middleware requests |

### Embedding Sidecar

| Variable | Type | Default | Description |
| --- | --- | --- | --- |
| `ISARTOR_EMBEDDING_SIDECAR__SIDECAR_URL` | `String` | `http://127.0.0.1:8082` | Base URL of the embedding sidecar (`/v1/embeddings`) |
| `ISARTOR_EMBEDDING_SIDECAR__MODEL_NAME` | `String` | `all-minilm` | Embedding model name |
| `ISARTOR_EMBEDDING_SIDECAR__TIMEOUT_SECONDS` | `u64` | `10` | HTTP request timeout for embedding calls |

### Layer 3 — External Cloud LLM

| Variable | Type | Default | Description |
| --- | --- | --- | --- |
| `ISARTOR_LLM_PROVIDER` | `String` | `openai` | Provider: `openai`, `azure`, `anthropic`, `xai` |
| `ISARTOR_EXTERNAL_LLM_URL` | `String` | `https://api.openai.com/v1/chat/completions` | Base URL for the external LLM API |
| `ISARTOR_EXTERNAL_LLM_MODEL` | `String` | `gpt-4o-mini` | Model name to request |
| `ISARTOR_EXTERNAL_LLM_API_KEY` | `String` | *(empty)* | API key for the cloud LLM provider |

### Azure OpenAI (Layer 3)

| Variable | Type | Default | Description |
| --- | --- | --- | --- |
| `ISARTOR_AZURE_DEPLOYMENT_ID` | `String` | *(empty)* | Azure OpenAI deployment ID (only when `llm_provider=azure`) |
| `ISARTOR_AZURE_API_VERSION` | `String` | `2024-08-01-preview` | Azure OpenAI API version |

### Observability

| Variable | Type | Default | Description |
| --- | --- | --- | --- |
| `ISARTOR_ENABLE_MONITORING` | `bool` | `false` | Enable OpenTelemetry trace and metric export |
| `ISARTOR_OTEL_EXPORTER_ENDPOINT` | `String` | `http://localhost:4317` | OTel Collector gRPC endpoint |

### Pipeline v2 — Algorithmic Gateway Tuning

| Variable | Type | Default | Description |
| --- | --- | --- | --- |
| `ISARTOR_PIPELINE_EMBEDDING_DIM` | `u64` | `384` | Embedding vector dimension (must match model: 384 for MiniLM, 768 for nomic-embed-text, 1024 for mxbai-embed-large) |
| `ISARTOR_PIPELINE_SIMILARITY_THRESHOLD` | `f64` | `0.92` | Cosine similarity threshold for the v2 pipeline semantic cache |
| `ISARTOR_PIPELINE_RERANK_TOP_K` | `u64` | `5` | Number of top-K documents to keep after reranking (Layer 2.5) |
| `ISARTOR_PIPELINE_MAX_CONCURRENCY` | `u64` | `256` | Maximum concurrency limit (ceiling) for the adaptive limiter |
| `ISARTOR_PIPELINE_MIN_CONCURRENCY` | `u64` | `4` | Minimum concurrency limit (floor) for the adaptive limiter |
| `ISARTOR_PIPELINE_TARGET_LATENCY_MS` | `u64` | `500` | Target P95 latency in ms for the adaptive concurrency algorithm (AIMD) |

### Embedded Classifier (Compiled Defaults)

These are set in `EmbeddedClassifierConfig::default()` in `src/services/local_inference.rs`. They are not currently configurable via env vars — override by modifying the source.

| Setting | Default | Description |
| --- | --- | --- |
| `repo_id` | `mradermacher/gemma-2-2b-it-GGUF` | Hugging Face repository for the GGUF model |
| `gguf_filename` | `gemma-2-2b-it.Q4_K_M.gguf` | Model file (~1.5 GB download) |
| `max_classify_tokens` | `20` | Maximum tokens for classification output |
| `max_generate_tokens` | `256` | Maximum tokens for simple task execution |
| `temperature` | `0.0` | Sampling temperature (0.0 = greedy, deterministic) |
| `repetition_penalty` | `1.1` | Repetition penalty factor |

### Rust Logging

| Variable | Default | Description |
| --- | --- | --- |
| `RUST_LOG` | *(unset)* | Standard `env_logger` / `tracing` filter (e.g., `isartor=info`, `isartor=debug,tower_http=trace`) |

---

## TOML Config File Example

Place as `isartor.toml` in the working directory:

```toml
# isartor.toml — Level 2 sidecar configuration

host_port = "0.0.0.0:8080"
gateway_api_key = "my-production-key"

# Layer 1 — Cache
cache_mode = "both"
embedding_model = "all-minilm"
similarity_threshold = 0.85
cache_ttl_secs = 600
cache_max_capacity = 50000

# Layer 2 — Generation Sidecar
[layer2]
sidecar_url = "http://127.0.0.1:8081"
model_name = "phi-3-mini"
timeout_seconds = 30

# Embedding Sidecar
[embedding_sidecar]
sidecar_url = "http://127.0.0.1:8082"
model_name = "all-minilm"
timeout_seconds = 10

# Layer 3 — Cloud LLM
llm_provider = "openai"
external_llm_url = "https://api.openai.com/v1/chat/completions"
external_llm_model = "gpt-4o-mini"
external_llm_api_key = ""  # Prefer env var for secrets

# Observability
enable_monitoring = true
otel_exporter_endpoint = "http://localhost:4317"

# Pipeline v2
pipeline_embedding_dim = 384
pipeline_similarity_threshold = 0.92
pipeline_rerank_top_k = 5
pipeline_max_concurrency = 256
pipeline_min_concurrency = 4
pipeline_target_latency_ms = 500
```

---

## Per-Tier Recommended Defaults

### Level 1 — Minimal (Edge / VPS)

```bash
ISARTOR_CACHE_MODE=exact              # No embedding sidecar
ISARTOR_CACHE_TTL_SECS=300
ISARTOR_CACHE_MAX_CAPACITY=5000       # Smaller memory footprint
ISARTOR_ENABLE_MONITORING=false       # No collector running
ISARTOR_PIPELINE_MAX_CONCURRENCY=64   # Single-machine limits
ISARTOR_PIPELINE_MIN_CONCURRENCY=2
ISARTOR_PIPELINE_TARGET_LATENCY_MS=1000
```

### Level 2 — Sidecar (Docker Compose)

```bash
ISARTOR_CACHE_MODE=both               # Semantic cache enabled
ISARTOR_CACHE_TTL_SECS=300
ISARTOR_CACHE_MAX_CAPACITY=10000
ISARTOR_ENABLE_MONITORING=true
ISARTOR_OTEL_EXPORTER_ENDPOINT=http://otel-collector:4317
ISARTOR_PIPELINE_MAX_CONCURRENCY=256
ISARTOR_PIPELINE_MIN_CONCURRENCY=4
ISARTOR_PIPELINE_TARGET_LATENCY_MS=500
```

### Level 3 — Enterprise (Kubernetes)

```bash
ISARTOR_CACHE_MODE=both
ISARTOR_CACHE_TTL_SECS=600            # Longer TTL, more cache value
ISARTOR_CACHE_MAX_CAPACITY=100000     # Large cache for high traffic
ISARTOR_ENABLE_MONITORING=true
ISARTOR_OTEL_EXPORTER_ENDPOINT=http://otel-collector.isartor:4317
ISARTOR_PIPELINE_MAX_CONCURRENCY=512  # Higher concurrency ceiling
ISARTOR_PIPELINE_MIN_CONCURRENCY=8
ISARTOR_PIPELINE_TARGET_LATENCY_MS=300  # Tighter latency target
```

---

## Provider-Specific Configuration

### OpenAI

```bash
ISARTOR_LLM_PROVIDER=openai
ISARTOR_EXTERNAL_LLM_URL=https://api.openai.com/v1/chat/completions
ISARTOR_EXTERNAL_LLM_MODEL=gpt-4o-mini
ISARTOR_EXTERNAL_LLM_API_KEY=sk-...
```

### Azure OpenAI

```bash
ISARTOR_LLM_PROVIDER=azure
ISARTOR_EXTERNAL_LLM_URL=https://your-resource.openai.azure.com
ISARTOR_EXTERNAL_LLM_MODEL=gpt-4o-mini
ISARTOR_EXTERNAL_LLM_API_KEY=your-azure-key
ISARTOR_AZURE_DEPLOYMENT_ID=your-deployment-name
ISARTOR_AZURE_API_VERSION=2024-08-01-preview
```

### Anthropic

```bash
ISARTOR_LLM_PROVIDER=anthropic
ISARTOR_EXTERNAL_LLM_URL=https://api.anthropic.com/v1/messages
ISARTOR_EXTERNAL_LLM_MODEL=claude-3-5-sonnet-20241022
ISARTOR_EXTERNAL_LLM_API_KEY=sk-ant-...
```

### xAI / Grok

```bash
ISARTOR_LLM_PROVIDER=xai
ISARTOR_EXTERNAL_LLM_URL=https://api.x.ai/v1/chat/completions
ISARTOR_EXTERNAL_LLM_MODEL=grok-2
ISARTOR_EXTERNAL_LLM_API_KEY=xai-...
```

---

*← Back to [README](../README.md)*
