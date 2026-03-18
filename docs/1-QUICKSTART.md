
# Isartor Quickstart

Get started with Isartor in seconds using one of the following methods.

## Path A: Docker (Recommended)

```bash
docker run -p 8080:8080 ghcr.io/isartor-ai/isartor:latest
```

Verify:

```bash
curl http://localhost:8080/health
```

## Path B: macOS & Linux (Binary)

```bash
curl -fsSL https://raw.githubusercontent.com/isartor-ai/Isartor/main/install.sh | sh
```

## Path C: Windows (Binary)

```powershell
irm https://raw.githubusercontent.com/isartor-ai/Isartor/main/install.ps1 | iex
```

---

## Test the Prompt Firewall

```bash
curl -s http://localhost:8080/api/chat \
  -H "Content-Type: application/json" \
  -d '{"prompt": "Calculate 2+2"}'
```

---

### Request 1: Complex Prompt (routes to OpenAI)

```bash
curl -X POST http://localhost:8080/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "gemma-2-2b-it",
    "messages": [
      {"role": "user", "content": "Explain the quantum Hall effect in detail, including its significance for condensed matter physics and any applications in modern technology."}
    ]
  }'
```

**Expected JSON Response (snippet):**
```json
{
  "id": "chatcmpl-abc123",
  "object": "chat.completion",
  "choices": [
    {
      "message": {
        "role": "assistant",
        "content": "The quantum Hall effect is a phenomenon..."
      }
    }
  ],
  "usage": { ... }
}
```

**Console Log (snippet):**
```
INFO  [slm_triage] Layer 3 fallback: OpenAI
INFO  [cache] Layer 1a miss: quantum Hall effect prompt
```

### Request 2: Same Prompt (demonstrates Layer 1a Exact Cache hit)

```bash
curl -X POST http://localhost:8080/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "gemma-2-2b-it",
    "messages": [
      {"role": "user", "content": "Explain the quantum Hall effect in detail, including its significance for condensed matter physics and any applications in modern technology."}
    ]
  }'
```

**Expected JSON Response (snippet):**
```json
{
  "id": "chatcmpl-abc123",
  "object": "chat.completion",
  "choices": [
    {
      "message": {
        "role": "assistant",
        "content": "The quantum Hall effect is a phenomenon..."
      }
    }
  ],
  "usage": { ... }
}
```

**Console Log (snippet):**
```
INFO  [cache] Layer 1a exact match: quantum Hall effect prompt
INFO  [slm_triage] Short-circuit: cache hit
```

---

For advanced configuration, see [README.md](../README.md) and [docs/2-ARCHITECTURE.md](2-ARCHITECTURE.md).
