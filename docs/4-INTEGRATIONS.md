# Isartor Integration Guide

Isartor is a prompt firewall that can act as a **drop-in OpenAI-compatible gateway** (and a minimal Anthropic-compatible gateway) for common SDKs and agent frameworks.

## Endpoints

Isartor’s server defaults to: `http://localhost:8080`.

Authenticated chat endpoints:

- **Native Isartor** (recommended for direct use)
  - `POST /api/chat`
  - `POST /api/v1/chat` (alias)
- **OpenAI Chat Completions compatible**
  - `POST /v1/chat/completions`
- **Anthropic Messages compatible**
  - `POST /v1/messages`

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

Isartor ships a helper CLI to configure popular clients to route through the gateway.

```bash
# Show what’s connected and test the gateway
isartor connect status --gateway-url http://localhost:8080

# Claude Code (CONNECT proxy + TLS MITM)
isartor connect claude --gateway-url http://localhost:8080

# GitHub Copilot CLI (CONNECT proxy + TLS MITM — see below)
isartor connect copilot --gateway-url http://localhost:8080

# Antigravity (CONNECT proxy + TLS MITM)
isartor connect antigravity --gateway-url http://localhost:8080
```

Add `--gateway-api-key <key>` to these commands only if you have explicitly enabled gateway auth.

### GitHub Copilot CLI (CONNECT Proxy)

Copilot CLI, Claude Code, and Antigravity can be routed through Isartor's HTTP CONNECT
proxy so Isartor can preserve each client's native upstream as Layer 3 while still
deflecting requests locally at L1/L2 when possible.

#### Prerequisites

- Isartor installed (`curl -fsSL https://raw.githubusercontent.com/isartor-ai/Isartor/main/install.sh | sh`)
- GitHub Copilot CLI installed (`gh extension install github/gh-copilot`)

#### Step-by-step setup

```bash
# 1. Start Isartor with the Copilot CONNECT proxy
#    This starts both the API gateway (:8080) and the CONNECT proxy (:8081).
#    Ports are configurable — see "Custom ports" below.
isartor up copilot

# 2. Generate the shell environment file for Copilot
#    Writes ~/.isartor/env/copilot.sh with HTTPS_PROXY and NODE_EXTRA_CA_CERTS.
#    The command auto-detects the running proxy port from your config, so
#    it matches even if you changed ISARTOR__PROXY_PORT.
isartor connect copilot

# 3. Activate the proxy environment in your current shell
#    IMPORTANT: run this in every new shell where you want Copilot to
#    route through Isartor.
source ~/.isartor/env/copilot.sh

# 4. Use Copilot normally — traffic now routes through Isartor
gh copilot suggest "explain this function"
```

> **Order matters.** Always run `isartor up copilot` *before* `isartor connect copilot`.
> The `connect` command tests the gateway to confirm it is reachable.

#### Custom ports

Override the default ports with environment variables:

```bash
export ISARTOR__HOST_PORT=127.0.0.1:18080    # gateway
export ISARTOR__PROXY_PORT=127.0.0.1:18081   # CONNECT proxy
isartor up copilot

# connect auto-detects the port from config — no extra flags needed
isartor connect copilot
source ~/.isartor/env/copilot.sh
```

You can also pass `--proxy-port` explicitly to override:

```bash
isartor connect copilot --proxy-port 127.0.0.1:18081
```

#### Verifying the connection

```bash
# Check connection status
isartor connect status

# View recent proxy decisions
curl -s http://localhost:8080/debug/proxy/recent | jq .
```

#### Stopping and disconnecting

```bash
# Stop the Isartor server
isartor stop

# Remove the Copilot proxy configuration
isartor connect copilot --disconnect

# Unset env vars in the current shell (or just open a new shell)
unset HTTPS_PROXY NODE_EXTRA_CA_CERTS ISARTOR_COPILOT_ENABLED
```

#### Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| Copilot hangs or "connection refused" | `HTTPS_PROXY` points to wrong port | Re-run `isartor connect copilot` and `source ~/.isartor/env/copilot.sh` |
| Copilot works but bypasses Isartor | Env vars not loaded in the shell | Run `source ~/.isartor/env/copilot.sh` in the same shell that launches Copilot |
| TLS / certificate errors | CA not trusted by Node.js | Verify `NODE_EXTRA_CA_CERTS` points to `~/.isartor/ca/isartor-ca.pem` |
| "address already in use" on startup | Previous Isartor still running | Run `isartor stop` first, or check `lsof -i :8081` |

If you start Copilot from a different shell or from an already-running process
that did not inherit the generated env vars, traffic will bypass Isartor and
you will not see proxy entries in `isartor connect status` or
`/debug/proxy/recent`.

#### How it works

1. A local CA certificate is generated at `~/.isartor/ca/isartor-ca.pem`
2. The CONNECT proxy runs on `:8081` only when you start `isartor up <client>` (configurable via `ISARTOR__PROXY_PORT`)
3. `NODE_EXTRA_CA_CERTS` tells Copilot's Node.js runtime to trust the local CA
4. The proxy intercepts CONNECT requests to known Copilot, Anthropic, and Antigravity domains
5. TLS is terminated with a leaf certificate signed by the local CA
6. POST requests to `/v1/chat/completions` and `/v1/messages` are routed through the Deflection Stack (L1a → L1b → L2 → client upstream L3)
7. All other traffic is tunnelled transparently

**Intercepted domains:**
- `copilot-proxy.githubusercontent.com`
- `api.github.com`
- `api.individual.githubcopilot.com`
- `api.business.githubcopilot.com`
- `api.enterprise.githubcopilot.com`
- `api.anthropic.com`
- `cloudcode-pa.googleapis.com`
- `daily-cloudcode-pa.googleapis.com`
- `daily-cloudcode-pa.sandbox.googleapis.com`

#### Notes

- The CA is only trusted by Node.js (via `NODE_EXTRA_CA_CERTS`). No system-level trust changes are made.
- Claude Code and Antigravity integrations also export `SSL_CERT_FILE` / `REQUESTS_CA_BUNDLE` for non-Node HTTPS stacks.
- Some tools support overriding the OpenAI base URL directly (preferred). Point them at `http://localhost:8080/v1`.


---

For more details, see [README.md](../README.md) and [docs/2-ARCHITECTURE.md](2-ARCHITECTURE.md).
