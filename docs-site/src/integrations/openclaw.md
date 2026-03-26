# OpenClaw

[OpenClaw](https://github.com/openclaw/openclaw) is a self-hosted AI assistant that can connect chat apps and agent workflows to LLM providers. The pragmatic Isartor setup is to register Isartor as a custom OpenAI-compatible OpenClaw provider and let OpenClaw use that provider as its primary model path.

This is similar in spirit to the LiteLLM integration docs, but with one important difference:

- **LiteLLM** is a multi-model gateway and catalog
- **Isartor** is a prompt firewall / gateway that currently exposes the upstream model you configured in Isartor itself

So the best OpenClaw UX is: **configure the model in Isartor first, then let `isartor connect openclaw` mirror that model into OpenClaw's provider config.**

## Pragmatic setup

```bash
# 1. Configure Isartor's upstream provider/model
isartor set-key -p groq
isartor check

# 2. Start Isartor
isartor up --detach

# 3. Make sure OpenClaw is onboarded
openclaw onboard --install-daemon

# 4. Register Isartor as an OpenClaw provider
isartor connect openclaw

# 5. Verify OpenClaw sees the provider/model
openclaw models status

# 6. Smoke test a prompt
openclaw agent --agent main -m "Hello from OpenClaw through Isartor"
```

## What `isartor connect openclaw` does

It writes or updates your OpenClaw config (default: `~/.openclaw/openclaw.json`) with:

1. `models.providers.isartor`
2. a single managed model entry matching Isartor's current upstream model
3. `agents.defaults.model.primary = "isartor/<your-model>"`

Example generated provider block:

```json5
models: {
  providers: {
    isartor: {
      baseUrl: "http://localhost:8080/v1",
      apiKey: "isartor-local",
      api: "openai-completions",
      models: [
        {
          id: "openai/gpt-oss-120b",
          name: "Isartor (openai/gpt-oss-120b)"
        }
      ]
    }
  }
}
```

And the default model becomes:

```json5
agents: {
  defaults: {
    model: {
      primary: "isartor/openai/gpt-oss-120b"
    }
  }
}
```

## Why this is the best fit

The upstream LiteLLM/OpenClaw docs assume the gateway can expose a multi-model catalog and route among many providers behind one endpoint.

Isartor is different today:

- OpenClaw talks to Isartor over the OpenAI-compatible `/v1/chat/completions` surface
- Isartor forwards using **its configured upstream provider/model**
- OpenClaw model refs should therefore mirror the model currently configured in Isartor

That means:

- if you change Isartor's provider/model later, rerun `isartor connect openclaw`
- do **not** expect `isartor/openai/...` and `isartor/anthropic/...` fallbacks to behave like LiteLLM provider switching unless Isartor itself grows multi-provider routing later

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--model` | Isartor's configured upstream model | Override the single model ID exposed to OpenClaw |
| `--config-path` | auto-detected | Path to `openclaw.json` |
| `--gateway-api-key` | (none) | Gateway key if auth is enabled |

## Files written

- `~/.openclaw/openclaw.json` — managed OpenClaw provider config
- `openclaw.json.isartor-backup` — backup, when a prior config existed

## Disconnecting

```bash
isartor connect openclaw --disconnect
```

If a backup exists, Isartor restores it. Otherwise it removes only the managed `models.providers.isartor` entry and related `isartor/...` default-model references.

## Recommended user workflow

For day-to-day use:

1. Pick your upstream provider with `isartor set-key`
2. Validate with `isartor check`
3. Keep Isartor running with `isartor up --detach`
4. Let OpenClaw use `isartor/<configured-model>` as its primary model
5. Use `openclaw models status` whenever you want to confirm what OpenClaw currently sees

If you later switch Isartor from, for example, Groq to OpenAI or Azure:

```bash
isartor set-key -p openai
isartor check
isartor connect openclaw
```

That refreshes OpenClaw's provider model to match the new Isartor config.

## What Isartor does for OpenClaw

| Benefit | How |
|---------|-----|
| **Cache repeated agent prompts** | OpenClaw often repeats the same context and system framing. L1a exact cache resolves those instantly. |
| **Catch paraphrases** | L1b semantic cache resolves similar follow-ups locally when safe. |
| **Compress repeated instructions** | L2.5 trims repeated context before cloud fallback. |
| **Keep one stable gateway URL** | OpenClaw only needs `isartor/<model>` while Isartor owns the upstream provider configuration. |
| **Observability** | `isartor stats --by-tool` lets you track OpenClaw cache hits, latency, and savings. |

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| OpenClaw cannot reach the provider | Isartor not running | Run `isartor up --detach` first |
| OpenClaw still shows the old model | Isartor model changed after initial connect | Re-run `isartor connect openclaw` |
| Auth errors (401) | Gateway auth enabled | Re-run with `--gateway-api-key` or set `ISARTOR__GATEWAY_API_KEY` |
| "Model is not allowed" | OpenClaw allowlist still excludes the managed model | Re-run `isartor connect openclaw` so the managed model is re-added to the allowlist |
