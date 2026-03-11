# 📄 Architecture Decision Records

> **Key design decisions, trade-offs, and rationale behind Isartor's architecture.**

Each ADR follows a lightweight format: Context → Decision → Consequences.

---

## ADR-001: Multi-Layer Pipeline Architecture

**Date:** 2024 · **Status:** Accepted

### Context

AI gateway traffic follows a power-law distribution: the majority of prompts are simple or repetitive, while only a small fraction requires expensive cloud LLMs. Sending all traffic to a single provider wastes tokens and money.

### Decision

Implement a **sequential pipeline** with 4+ layers, each capable of short-circuiting:

- **Layer 0** — Operational defense (auth, rate limiting, concurrency control)
- **Layer 1** — Semantic + exact cache (zero-cost hits)
- **Layer 2** — Local SLM triage (classify intent, execute simple tasks locally)
- **Layer 2.5** — Context optimiser (retrieve + rerank to minimise token usage)
- **Layer 3** — Cloud LLM fallback (only the hardest prompts)

### Consequences

- **Positive:** 60–80% of traffic can be resolved before Layer 3, dramatically reducing cost.
- **Positive:** Each layer adds latency only when needed — cache hits are sub-millisecond.
- **Positive:** Clear separation of concerns; each layer is independently testable.
- **Negative:** Pipeline adds conceptual complexity vs. a simple reverse proxy.
- **Negative:** Each layer needs its own error handling and timeout strategy.

---

## ADR-002: Axum + Tokio as Runtime Foundation

**Date:** 2024 · **Status:** Accepted

### Context

The gateway must handle high concurrency (thousands of simultaneous connections) with low latency overhead. The binary should be small, statically linked, and deployable to minimal environments.

### Decision

Use **Axum 0.8** on **Tokio 1.x** for the async HTTP server. Build with `--target x86_64-unknown-linux-musl` and `opt-level = "z"` + LTO for a ~5 MB static binary.

### Consequences

- **Positive:** Tokio's work-stealing scheduler handles 10K+ concurrent connections efficiently.
- **Positive:** Axum's type-safe extractors catch errors at compile time.
- **Positive:** Static musl binary runs in distroless containers (no libc, no shell).
- **Negative:** Rust's compilation times are longer than Go/Node.js equivalents.
- **Negative:** Ecosystem is smaller — fewer off-the-shelf middleware components.

---

## ADR-003: Embedded Candle Classifier (Layer 2)

**Date:** 2024 · **Status:** Accepted

### Context

For minimal deployments (edge, VPS, air-gapped), requiring an external sidecar (llama.cpp, Ollama, TGI) adds operational complexity. Many classification tasks can be handled by a 2B parameter model on CPU.

### Decision

Embed a **Gemma-2-2B-IT** GGUF model directly in the Rust process using the [candle](https://github.com/huggingface/candle) framework. The model is loaded on first start via `hf-hub` (auto-downloaded from Hugging Face) and wrapped in a `tokio::sync::Mutex` for thread-safe inference on `spawn_blocking`.

### Consequences

- **Positive:** Zero external dependencies for Layer 2 classification — a single binary handles everything.
- **Positive:** No HTTP overhead for classification calls; inference is an in-process function call.
- **Positive:** Works in air-gapped environments with pre-cached models.
- **Negative:** ~1.5 GB memory overhead for the Q4_K_M model weights.
- **Negative:** CPU inference is slower than GPU (50–200 ms classification, 200–2000 ms generation).
- **Negative:** `Mutex` serialises inference calls — throughput limited to one inference at a time.
- **Trade-off:** For higher throughput, upgrade to Level 2 (llama.cpp sidecar on GPU).

---

## ADR-004: Three Deployment Tiers

**Date:** 2024 · **Status:** Accepted

### Context

Isartor targets a wide range of deployments, from a developer's laptop to enterprise Kubernetes clusters. A single deployment model cannot serve all use cases optimally.

### Decision

Define three explicit deployment tiers that share the **same binary and configuration surface**:

| Tier | Strategy | Target |
| --- | --- | --- |
| **Level 1** | Monolithic binary, embedded candle | VPS, edge, bare metal |
| **Level 2** | Gateway + llama.cpp sidecars | Docker Compose, single host + GPU |
| **Level 3** | Stateless pods + inference pools | Kubernetes, Helm, HPA |

The tier is selected purely by environment variables and infrastructure, not by code changes.

### Consequences

- **Positive:** A single codebase and binary serves all deployment scenarios.
- **Positive:** Users start at Level 1 and upgrade incrementally — no migrations.
- **Positive:** Clear documentation entry points for each tier.
- **Negative:** Some config variables are irrelevant at certain tiers (e.g., `ISARTOR__LAYER2__SIDECAR_URL` is unused at Level 1 with embedded candle).
- **Negative:** Testing all three tiers requires different infrastructure setups.

---

## ADR-005: llama.cpp as Sidecar (Level 2) Instead of Ollama

**Date:** 2024 · **Status:** Accepted

### Context

The original design used [Ollama](https://ollama.com/) (~1.5 GB image) as the local SLM engine. While Ollama has a convenient API and model management, it's heavyweight for a sidecar.

### Decision

Replace Ollama with **llama.cpp server** (`ghcr.io/ggml-org/llama.cpp:server`, ~30 MB) as the default sidecar in `docker-compose.sidecar.yml`. Two instances run side by side:

- **slm-generation** (port 8081) — Phi-3-mini for classification and generation
- **slm-embedding** (port 8082) — all-MiniLM-L6-v2 with `--embedding` flag

### Consequences

- **Positive:** 50× smaller container images (30 MB vs. 1.5 GB).
- **Positive:** Faster cold starts; no model pull step needed (uses `--hf-repo` auto-download).
- **Positive:** OpenAI-compatible API — gateway code doesn't need to change.
- **Negative:** Ollama's model management UX (pull, list, delete) is lost.
- **Negative:** Each model needs its own llama.cpp instance (no multi-model serving).
- **Migration:** Ollama-based Compose files (`docker-compose.yml`, `docker-compose.azure.yml`) are retained for backward compatibility.
- **Update (ADR-011):** The **slm-embedding** sidecar (port 8082) is now **optional** for the v1 middleware pipeline. Layer 1 semantic cache embeddings are generated in-process via fastembed (ONNX Runtime). The embedding sidecar is only required for the v2 algorithmic pipeline (`/api/v2/chat`).

---

## ADR-006: rig-core for Multi-Provider LLM Client

**Date:** 2024 · **Status:** Accepted

### Context

Layer 3 must route to multiple cloud LLM providers (OpenAI, Azure OpenAI, Anthropic, xAI). Implementing each provider's API client from scratch would be error-prone and hard to maintain.

### Decision

Use [rig-core](https://crates.io/crates/rig-core) (v0.32.0) as the unified LLM client. Rig provides a consistent `CompletionModel` abstraction over all supported providers.

### Consequences

- **Positive:** Single configuration surface (`ISARTOR__LLM_PROVIDER` + `ISARTOR__EXTERNAL_LLM_API_KEY`) switches providers.
- **Positive:** Provider-specific quirks (Azure deployment IDs, Anthropic versioning) handled by rig.
- **Negative:** Adds a dependency; rig's release cadence may not match our needs.
- **Negative:** Limited to providers rig supports (but covers all major ones).

---

## ADR-007: AIMD Adaptive Concurrency Control

**Date:** 2024 · **Status:** Accepted

### Context

A fixed concurrency limit either over-provisions (wasting resources) or under-provisions (rejecting requests during traffic spikes). The gateway needs to dynamically adjust its limit based on real-time latency.

### Decision

Implement an **Additive Increase / Multiplicative Decrease (AIMD)** concurrency limiter at Layer 0:

- If P95 latency < target → `limit += 1` (additive increase).
- If P95 latency > target → `limit *= 0.5` (multiplicative decrease).
- Bounded by `ISARTOR__PIPELINE_MIN_CONCURRENCY` and `ISARTOR__PIPELINE_MAX_CONCURRENCY`.

### Consequences

- **Positive:** Self-tuning: the limit converges to the optimal value for the current load.
- **Positive:** Protects downstream services (sidecars, cloud LLMs) from overload.
- **Negative:** During cold start, the limit starts low and ramps up — initial requests may see 503s.
- **Tuning:** `ISARTOR__PIPELINE_TARGET_LATENCY_MS` must be calibrated per deployment tier.

---

## ADR-008: Dual API Surface (v1 + v2)

**Date:** 2024 · **Status:** Accepted

### Context

The original v1 API used Axum middleware for pipeline layers. As complexity grew, a purpose-built orchestrator was needed for the algorithmic pipeline.

### Decision

Maintain both API versions:

- **v1** (`/api/chat`) — Middleware-based pipeline (original). Each layer is an Axum middleware.
- **v2** (`/api/v2/chat`) — Orchestrator-based pipeline with explicit `PipelineContext`, trait-based components, and structured `processing_log` in responses.

### Consequences

- **Positive:** v1 remains available for backward compatibility.
- **Positive:** v2's orchestrator pattern is easier to test, extend, and observe.
- **Negative:** Two code paths to maintain until v1 is deprecated.
- **Plan:** Deprecate v1 in a future release once v2 is battle-tested.

---

## ADR-009: Distroless Container Image

**Date:** 2024 · **Status:** Accepted

### Context

The gateway binary is statically linked (musl). The runtime container only needs to execute a single binary.

### Decision

Use `gcr.io/distroless/static-debian12` as the runtime base image. It contains no shell, no package manager, no libc — only the static binary.

### Consequences

- **Positive:** Minimal attack surface — no shell to exec into, no tools for attackers.
- **Positive:** Tiny image size (base ~2 MB + binary ~5 MB = ~7 MB total).
- **Positive:** Passes most container security scanners with zero CVEs.
- **Negative:** Cannot `docker exec` into the container for debugging (no shell).
- **Negative:** Cannot install additional tools at runtime.
- **Workaround:** Use `docker logs`, Jaeger traces, and Prometheus metrics for debugging.

---

## ADR-010: OpenTelemetry for Observability

**Date:** 2024 · **Status:** Accepted

### Context

The gateway needs distributed tracing and metrics. Vendor-specific SDKs (Datadog, New Relic, etc.) create lock-in.

### Decision

Use **OpenTelemetry** (OTLP gRPC) as the sole telemetry interface. Traces and metrics are exported to an OTel Collector, which can forward to any backend (Jaeger, Prometheus, Grafana, Datadog, etc.).

### Consequences

- **Positive:** Vendor-neutral — switch backends by reconfiguring the collector, not the app.
- **Positive:** OTLP is a CNCF standard with wide ecosystem support.
- **Positive:** When `ISARTOR__ENABLE_MONITORING=false`, no OTel SDK is initialised — zero overhead.
- **Negative:** Requires an OTel Collector as middleware (adds one more service in Level 2/3).
- **Negative:** Auto-instrumentation is less mature in Rust than in Java/Python.

---

## ADR-011: fastembed for In-Process Sentence Embeddings

| | |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2025-06 |
| **Deciders** | Core team |
| **Relates to** | ADR-003 (Embedded Candle), ADR-005 (llama.cpp sidecar) |

### Context

Layer 1 (semantic cache) must generate sentence embeddings for every incoming prompt to compute cosine similarity against the vector cache. Previously, this was done via an HTTP call to a llama.cpp sidecar running all-MiniLM-L6-v2, adding ~2–5 ms of network latency per request on the hot path — plus the operational complexity of managing a separate container.

### Decision

Use **fastembed** (`fastembed = "5.12.0"`) to run **BAAI/bge-small-en-v1.5** in-process via ONNX Runtime. The embedding model (~33 MB) is downloaded once on first startup and cached in `.fastembed_cache/`. Inference is invoked through `tokio::task::spawn_blocking` since ONNX execution is CPU-bound.

- **Model:** BAAI/bge-small-en-v1.5 — 384-dimensional embeddings, optimised for sentence similarity.
- **Runtime:** ONNX Runtime (bundled via fastembed) — no Python dependency, no sidecar.
- **Thread safety:** The inner `TextEmbedding` is wrapped in `std::sync::Mutex` because `embed()` takes `&mut self`. This is acceptable because inference is always called from `spawn_blocking`, never holding the lock across `.await` points.
- **Architecture:** `TextEmbedder` is initialised once at startup, stored as `Arc<TextEmbedder>` in `AppState`, and injected into the cache middleware.

### Alternatives Considered

| Alternative | Why rejected |
| --- | --- |
| llama.cpp sidecar (all-MiniLM-L6-v2) | Network round-trip on hot path, extra container to manage |
| candle for embeddings | candle excels at transformer generation but lacks a turnkey sentence-embedding API; fastembed wraps ONNX Runtime with a purpose-built embedding interface |
| sentence-transformers (Python) | Crosses FFI boundary, adds Python runtime dependency |
| ort (raw ONNX Runtime bindings) | Lower-level; fastembed provides model download, tokenisation, and batching out of the box |

### Consequences

- **Positive:** Eliminates ~2–5 ms network latency per embedding call on the cache hot path.
- **Positive:** Zero sidecar dependency for Level 1 — the minimal Dockerfile runs self-contained.
- **Positive:** Model is pinned to a specific version; reproducible builds.
- **Negative:** Binary size increases by ~5 MB (ONNX Runtime shared library).
- **Negative:** First startup downloads the model (~33 MB) if not pre-cached.
- **Negative:** `Mutex` serialises concurrent embedding calls within a single process (acceptable at current scale; can be replaced with a pool of models if needed).

---

## ADR-012: Pluggable Trait Provider (Hexagonal Architecture)

| | |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2025-06 |
| **Deciders** | Core team |
| **Relates to** | ADR-003 (Embedded Candle), ADR-004 (Three Deployment Tiers) |

### Context

As Isartor grew from a single-process binary (Level 1) to a multi-tier deployment (Level 1 → 2 → 3), the cache and SLM router components became tightly coupled to their in-process implementations. Scaling to Level 3 (Kubernetes, multiple replicas) requires:

1. **Shared cache** — in-process LRU caches are isolated per pod; cache hits are inconsistent, duplicating work.
2. **GPU-backed inference** — in-process Candle inference is CPU-bound; Level 3 needs a dedicated GPU inference pool (vLLM / TGI) that can scale independently.

Hard-coding these choices into the gateway binary would require compile-time feature flags or code branching, making the binary non-portable across tiers.

### Decision

Adopt the **Ports & Adapters (Hexagonal Architecture)** pattern:

- **Ports** (`src/core/ports.rs`) — Define `ExactCache` and `SlmRouter` as `async_trait` traits (`Send + Sync`), representing the interfaces the gateway depends on.
- **Adapters** (`src/adapters/`) — Provide concrete implementations:
  - `InMemoryCache` (ahash + LRU + parking_lot) and `RedisExactCache` for `ExactCache`
  - `EmbeddedCandleRouter` and `RemoteVllmRouter` for `SlmRouter`
- **Factory** (`src/factory.rs`) — `build_exact_cache(&config)` and `build_slm_router(&config, &http_client)` read `AppConfig.cache_backend` and `AppConfig.router_backend` at startup and return the appropriate `Box<dyn Trait>`.
- **Configuration** (`src/config.rs`) — `CacheBackend` enum (`Memory | Redis`) and `RouterBackend` enum (`Embedded | Vllm`) with associated connection URLs, selectable via `ISARTOR__CACHE_BACKEND` and `ISARTOR__ROUTER_BACKEND` env vars.

The **same binary** serves all three deployment tiers; the runtime behaviour is entirely configuration-driven.

### Alternatives Considered

| Alternative | Why rejected |
| --- | --- |
| Compile-time feature flags (`#[cfg(feature = "redis")]`) | Produces different binaries per tier; complicates CI and container builds |
| Service mesh sidecar (Envoy filter for caching) | Adds infrastructure complexity; cache logic is domain-specific |
| Plugin system (dynamic `.so` loading) | Over-engineered; `dyn Trait` with compile-time-known variants is simpler |
| Runtime scripting (Lua / Wasm policy) | Unnecessary indirection; Rust trait dispatch is zero-cost |

### Consequences

- **Positive:** One binary, all tiers — only env vars change between Level 1 (embedded everything) and Level 3 (Redis + vLLM).
- **Positive:** Horizontal scalability — with `cache_backend=redis`, all pods share the same cache; with `router_backend=vllm`, GPU inference scales independently.
- **Positive:** Testability — unit tests inject mock adapters via the trait interface.
- **Positive:** Extensibility — adding a new backend (e.g., Memcached, Triton) requires only a new adapter implementing the trait.
- **Negative:** Minor runtime overhead from `dyn Trait` dynamic dispatch (single vtable lookup per call — negligible vs. network I/O).
- **Negative:** Adapter skeletons (`RedisExactCache`, `RemoteVllmRouter`) are currently stubs returning `todo!()`; must be implemented before Level 3 production use.

---

*← Back to [README](../README.md)*
