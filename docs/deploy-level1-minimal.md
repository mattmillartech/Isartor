# 📄 Level 1 — Minimal Deployment (Edge / VPS / Bare Metal)

> **Single static binary, embedded candle inference, zero external dependencies.**

This guide covers deploying Isartor as a standalone process — no sidecars, no Docker Compose, no orchestrator. The gateway binary embeds a Gemma-2-2B-IT GGUF model via [candle](https://github.com/huggingface/candle) and handles classification + simple task execution entirely in-process.

---

## When to Use Level 1

| ✅ Good Fit | ❌ Consider Level 2/3 Instead |
| --- | --- |
| €5–€20/month VPS (Hetzner, DigitalOcean, Linode) | GPU inference for generation quality |
| ARM edge devices (Raspberry Pi 5, Jetson Nano) | More than ~50 concurrent users |
| Air-gapped / offline environments | Need embedding sidecar for semantic cache |
| Development & local experimentation | Production observability stack required |
| CI/CD test runners | Multi-node high-availability |

---

## Prerequisites

| Requirement | Minimum | Recommended |
| --- | --- | --- |
| **RAM** | 2 GB free | 4 GB free |
| **Disk** | 2 GB (model download) | 5 GB |
| **CPU** | 2 cores | 4+ cores (AVX2 recommended) |
| **Rust** (build from source) | 1.75+ | Latest stable |
| **OS** | Linux (x86_64 / aarch64), macOS | Ubuntu 22.04 LTS |

> **Memory budget:** Gemma-2-2B Q4_K_M ≈ 1.5 GB, tokenizer ≈ 4 MB, gateway runtime ≈ 50 MB. Total: ~1.6 GB resident.

---

## Option A: Build from Source

### 1. Clone & Build

```bash
git clone https://github.com/isartor-ai/isartor.git
cd isartor
cargo build --release
```

The release binary is at `./target/release/isartor` (~5 MB statically linked).

### 2. Configure Environment

Create a minimal `.env` file or export variables directly:

```bash
# Required — your cloud LLM key for Layer 3 fallback
export ISARTOR_EXTERNAL_LLM_API_KEY="sk-..."

# Optional — override defaults
export ISARTOR_GATEWAY_API_KEY="my-secret-key"
export ISARTOR_HOST_PORT="0.0.0.0:8080"
export ISARTOR_LLM_PROVIDER="openai"          # openai | azure | anthropic | xai
export ISARTOR_EXTERNAL_LLM_MODEL="gpt-4o-mini"

# Cache mode — in Level 1, "exact" is recommended since there's no
# embedding sidecar. Use "both" or "semantic" only if you bring your
# own embedding endpoint.
export ISARTOR_CACHE_MODE="exact"
```

### 3. Start the Gateway

```bash
./target/release/isartor
```

On first start, the embedded classifier will **auto-download** the Gemma-2-2B-IT GGUF model from Hugging Face Hub (~1.5 GB). Subsequent starts load from the local cache (`~/.cache/huggingface/`).

```
INFO  isartor > Listening on 0.0.0.0:8080
INFO  isartor::services::local_inference > Downloading model from mradermacher/gemma-2-2b-it-GGUF...
INFO  isartor::services::local_inference > Model loaded (1.5 GB), ready for inference
```

### 4. Verify

```bash
# Health check
curl http://localhost:8080/healthz
# {"status":"ok"}

# Test the pipeline
curl -s http://localhost:8080/api/v2/chat \
  -H "Content-Type: application/json" \
  -H "X-API-Key: my-secret-key" \
  -d '{"prompt": "Hello, how are you?"}' | jq .
```

---

## Option B: Docker (Single Container)

For environments where you prefer a container but don't need a full Compose stack.

### Build the Image

```bash
cd isartor
docker build -t isartor:latest -f docker/Dockerfile .
```

### Run

```bash
docker run -d \
  --name isartor \
  -p 8080:8080 \
  -e ISARTOR_GATEWAY_API_KEY="my-secret-key" \
  -e ISARTOR_EXTERNAL_LLM_API_KEY="sk-..." \
  -e ISARTOR_CACHE_MODE="exact" \
  -v isartor-models:/root/.cache/huggingface \
  isartor:latest
```

> **Note:** The `-v` flag mounts a named volume for the model cache so the ~1.5 GB GGUF download persists across container restarts.

> **Important:** The current Dockerfile produces a distroless image without the embedded candle model. For Level 1 container deployment with embedded inference, you may need a custom Dockerfile that includes the candle feature flags. See the [architecture decisions](architecture-decisions.md) for details.

---

## Option C: systemd Service (Production Linux)

For long-running production deployments on bare metal or VPS.

### 1. Install the Binary

```bash
# Build
cargo build --release

# Install to /usr/local/bin
sudo cp target/release/isartor /usr/local/bin/isartor
sudo chmod +x /usr/local/bin/isartor
```

### 2. Create a System User

```bash
sudo useradd --system --no-create-home --shell /usr/sbin/nologin isartor
```

### 3. Create Environment File

```bash
sudo mkdir -p /etc/isartor
sudo tee /etc/isartor/env <<'EOF'
ISARTOR_HOST_PORT=0.0.0.0:8080
ISARTOR_GATEWAY_API_KEY=your-production-key
ISARTOR_EXTERNAL_LLM_API_KEY=sk-...
ISARTOR_LLM_PROVIDER=openai
ISARTOR_EXTERNAL_LLM_MODEL=gpt-4o-mini
ISARTOR_CACHE_MODE=exact
RUST_LOG=isartor=info
EOF
sudo chmod 600 /etc/isartor/env
```

### 4. Create systemd Unit

```bash
sudo tee /etc/systemd/system/isartor.service <<'EOF'
[Unit]
Description=Isartor AI Orchestration Gateway
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=isartor
Group=isartor
EnvironmentFile=/etc/isartor/env
ExecStart=/usr/local/bin/isartor
Restart=on-failure
RestartSec=5
LimitNOFILE=65536

# Security hardening
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
ReadWritePaths=/var/cache/isartor

[Install]
WantedBy=multi-user.target
EOF
```

### 5. Create Model Cache Directory

```bash
sudo mkdir -p /var/cache/isartor
sudo chown isartor:isartor /var/cache/isartor
```

### 6. Enable & Start

```bash
sudo systemctl daemon-reload
sudo systemctl enable isartor
sudo systemctl start isartor

# Check status
sudo systemctl status isartor
sudo journalctl -u isartor -f
```

---

## Model Pre-Caching (Air-Gapped / Offline)

If the deployment target has no internet access, pre-download the model on a connected machine and copy it over.

### On the Connected Machine

```bash
# Install huggingface-cli
pip install huggingface-hub

# Download the GGUF file
huggingface-cli download mradermacher/gemma-2-2b-it-GGUF \
  gemma-2-2b-it.Q4_K_M.gguf \
  --local-dir ./models

# Also grab the tokenizer (from the base model)
huggingface-cli download google/gemma-2-2b-it \
  tokenizer.json \
  --local-dir ./models
```

### Transfer to Target

```bash
scp -r ./models/ user@target-host:/var/cache/isartor/
```

The embedded classifier checks `~/.cache/huggingface/` by default. Set `HF_HOME` or `HF_HUB_CACHE` to point to your pre-cached directory if needed.

---

## Level 1 Configuration Reference

These are the most relevant `ISARTOR_*` variables for Level 1 deployments. For the full reference, see [`docs/configuration.md`](configuration.md).

| Variable | Default | Level 1 Notes |
| --- | --- | --- |
| `ISARTOR_HOST_PORT` | `0.0.0.0:8080` | Bind address |
| `ISARTOR_GATEWAY_API_KEY` | `changeme` | **Change in production** |
| `ISARTOR_CACHE_MODE` | `both` | Use `exact` (no embedding sidecar available) |
| `ISARTOR_CACHE_TTL_SECS` | `300` | Cache TTL in seconds |
| `ISARTOR_CACHE_MAX_CAPACITY` | `10000` | Max entries per cache |
| `ISARTOR_LLM_PROVIDER` | `openai` | `openai` · `azure` · `anthropic` · `xai` |
| `ISARTOR_EXTERNAL_LLM_API_KEY` | *(empty)* | **Required** for Layer 3 fallback |
| `ISARTOR_EXTERNAL_LLM_MODEL` | `gpt-4o-mini` | Cloud LLM model name |
| `ISARTOR_ENABLE_MONITORING` | `false` | Enable for stdout OTel (no collector needed) |

### Embedded Classifier Defaults (Compiled)

| Setting | Default Value | Description |
| --- | --- | --- |
| `repo_id` | `mradermacher/gemma-2-2b-it-GGUF` | HF repo for the GGUF model |
| `gguf_filename` | `gemma-2-2b-it.Q4_K_M.gguf` | Model file (~1.5 GB) |
| `max_classify_tokens` | `20` | Token limit for classification |
| `max_generate_tokens` | `256` | Token limit for simple task execution |
| `temperature` | `0.0` | Greedy decoding for classification |
| `repetition_penalty` | `1.1` | Avoids degenerate loops |

---

## Performance Expectations

| Metric | Typical Value (4-core x86_64) |
| --- | --- |
| Cold start (model download) | 30–120 s (depends on bandwidth) |
| Warm start (cached model) | 3–8 s |
| Classification latency | 50–200 ms |
| Simple task execution | 200–2000 ms |
| Gateway overhead (no inference) | < 1 ms |
| Memory (steady state) | ~1.6 GB |
| Binary size | ~5 MB |

---

## Upgrading to Level 2

When your traffic outgrows Level 1, the migration path is straightforward:

1. **Switch cache mode** — `ISARTOR_CACHE_MODE=both` (semantic cache now available via sidecar).
2. **Add the embedding sidecar** — `ISARTOR_EMBEDDING_SIDECAR__SIDECAR_URL=http://127.0.0.1:8082`.
3. **Add the generation sidecar** — `ISARTOR_LAYER2__SIDECAR_URL=http://127.0.0.1:8081` (replaces embedded candle with the more powerful Phi-3-mini on GPU).
4. **Deploy via Docker Compose** — See [📄 `docs/deploy-level2-sidecar.md`](deploy-level2-sidecar.md).

No code changes required — only environment variables and infrastructure.

---

*← Back to [README](../README.md)*
