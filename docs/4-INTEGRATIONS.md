# Isartor Integration Guide

Isartor is an **OpenAI-compatible and Anthropic-compatible gateway** that deflects
repeated or simple prompts at Layer 1 (cache) and Layer 2 (local SLM) before they
reach the cloud. Clients integrate by **overriding their base URL** to point at
Isartor or by registering Isartor as an **MCP server** — no proxy, no MITM, no CA certificates.

## Endpoints

Isartor's server defaults to: `http://localhost:8080`.

Authenticated chat endpoints:

- **Native Isartor** (recommended for direct use)
  - `POST /api/chat`
  - `POST /api/v1/chat` (alias)
- **OpenAI Chat Completions compatible**
  - `POST /v1/chat/completions`
- **Anthropic Messages compatible**
  - `POST /v1/messages`
- **Copilot preToolUse hook** (legacy)
  - `POST /api/v1/hook/pretooluse`

## Authentication

Isartor can enforce a gateway key on authenticated routes when Layer 0 auth is enabled.

Supported headers:

- `X-API-Key: <gateway_api_key>`
- `Authorization: Bearer <gateway_api_key>` (useful for OpenAI/Anthropic-compatible clients)

By default, `gateway_api_key` is empty and **auth is disabled** (local-first). To enable gateway authentication, set `ISARTOR__GATEWAY_API_KEY` to a secret value. In production, **always** set a strong key.

## Observability headers

All endpoints in the Deflection Stack include:

- `X-Isartor-Layer`: `l1a` | `l1b` | `l2` | `l3` | `l0`
- `X-Isartor-Deflected`: `true` if resolved locally (no cloud call)

## Example: OpenAI-compatible request

```bash
curl -sS http://localhost:8080/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [
      {"role": "user", "content": "2 + 2?"}
    ]
  }'
```

If gateway auth is enabled, also add:

```bash
-H 'Authorization: Bearer your-secret-key'
```

## Example: Anthropic-compatible request

```bash
curl -sS http://localhost:8080/v1/messages \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "claude-sonnet-4-6",
    "system": "Be concise.",
    "max_tokens": 100,
    "messages": [
      {
        "role": "user",
        "content": [{"type": "text", "text": "What is the capital of France?"}]
      }
    ]
  }'
```

If gateway auth is enabled, also add:

```bash
-H 'X-API-Key: your-secret-key'
```

## Client integrations: `isartor connect …`

Isartor ships a helper CLI to configure popular clients to route through the
gateway. Each integration uses the lightest-weight mechanism available for that
client — an **MCP server** (for Copilot CLI) or a **base URL override**.

```bash
# Show what's connected and test the gateway
isartor connect status

# GitHub Copilot CLI (MCP server — isartor_chat tool)
isartor connect copilot

# Claude Code (base URL override)
isartor connect claude

# Antigravity (base URL override)
isartor connect antigravity

# OpenClaw (provider base URL)
isartor connect openclaw
```

Add `--gateway-api-key <key>` to these commands only if you have explicitly enabled gateway auth.

### GitHub Copilot CLI (MCP server)

Copilot CLI integrates via an **MCP (Model Context Protocol) server** that
Isartor registers as a stdio subprocess. Copilot gains an `isartor_chat` tool
whose prompts flow through the full deflection stack (L1a/L1b cache → L2 SLM →
L3 cloud), enabling cache hits and local deflection.

#### Prerequisites

- Isartor installed (`curl -fsSL https://raw.githubusercontent.com/isartor-ai/Isartor/main/install.sh | sh`)
- GitHub Copilot CLI installed

#### Step-by-step setup

```bash
# 1. Start Isartor
isartor up --detach

# 2. Register the MCP server with Copilot CLI
isartor connect copilot

# 3. Start Copilot normally — isartor_chat tool is now available
copilot
```

#### How it works

1. `isartor connect copilot` adds an `isartor` entry to `~/.copilot/mcp-config.json`
2. When Copilot CLI starts, it launches `isartor mcp` as a stdio subprocess
3. The MCP server exposes an `isartor_chat` tool
4. Prompts sent through `isartor_chat` go through the deflection stack:
   - **L1a** exact cache → **L1b** semantic cache → **L2** SLM triage → **L3** cloud
5. Repeated or similar prompts are deflected locally — zero cloud cost

#### Custom gateway URL

```bash
# If Isartor runs on a non-default port
isartor connect copilot --gateway-url http://localhost:18080
```

#### Disconnecting

```bash
isartor connect copilot --disconnect
```

This removes the `isartor` entry from `~/.copilot/mcp-config.json`.

### Claude Code (base URL override)

Claude Code integrates via `ANTHROPIC_BASE_URL`, pointing all API traffic at
Isartor's `/v1/messages` endpoint.

#### Step-by-step setup

```bash
# 1. Start Isartor
isartor up

# 2. Configure Claude Code
isartor connect claude

# 3. Claude Code now routes through Isartor automatically
```

#### How it works

1. `isartor connect claude` sets `ANTHROPIC_BASE_URL` in `~/.claude/settings.json`
2. Claude Code sends requests to Isartor's `/v1/messages` endpoint
3. Isartor forwards to the Anthropic API as Layer 3 when the request is not deflected

#### Disconnecting

```bash
isartor connect claude --disconnect
```

### Antigravity (base URL override)

Antigravity integrates via an environment file that sets the OpenAI base URL.

#### Step-by-step setup

```bash
# 1. Start Isartor
isartor up

# 2. Configure Antigravity
isartor connect antigravity

# 3. Source the env file
source ~/.isartor/env/antigravity.sh
```

#### How it works

1. `isartor connect antigravity` writes `OPENAI_BASE_URL` and `OPENAI_API_KEY` to `~/.isartor/env/antigravity.sh`
2. Antigravity sends requests to Isartor's `/v1/chat/completions` endpoint
3. Isartor forwards to the OpenAI-compatible upstream as Layer 3 when the request is not deflected

#### Disconnecting

```bash
isartor connect antigravity --disconnect
```

### OpenClaw (provider base URL)

OpenClaw integrates via a JSON patch to its provider configuration.

#### Step-by-step setup

```bash
# 1. Start Isartor
isartor up

# 2. Configure OpenClaw
isartor connect openclaw
```

#### How it works

1. `isartor connect openclaw` patches OpenClaw's provider config to set the base URL to Isartor
2. OpenClaw sends requests to Isartor's gateway
3. Isartor forwards to the configured upstream as Layer 3 when the request is not deflected

#### Disconnecting

```bash
isartor connect openclaw --disconnect
```

## Connection status

```bash
# Check all connected clients
isartor connect status
```

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| "connection refused" | Isartor not running | Run `isartor up` first |
| Copilot has no `isartor_chat` tool | MCP server not registered | Run `isartor connect copilot` |
| Copilot works but bypasses cache | Using native Copilot tools instead of `isartor_chat` | Ask Copilot to use the `isartor_chat` tool |
| Claude not routing through Isartor | `settings.json` not updated | Run `isartor connect claude` |
| Gateway returns 401 | Auth enabled but key not configured | Add `--gateway-api-key` to connect command |

---

For more details, see [README.md](../README.md) and [docs/2-ARCHITECTURE.md](2-ARCHITECTURE.md).
