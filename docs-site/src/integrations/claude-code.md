# Claude Code

Claude Code integrates via `ANTHROPIC_BASE_URL`, pointing all API traffic at
Isartor's `/v1/messages` endpoint.

## Step-by-step setup

```bash
# 1. Start Isartor
isartor up

# 2. Configure Claude Code
isartor connect claude

# 3. Claude Code now routes through Isartor automatically
```

## How it works

1. `isartor connect claude` sets `ANTHROPIC_BASE_URL` in `~/.claude/settings.json`
2. Claude Code sends requests to Isartor's `/v1/messages` endpoint
3. Isartor forwards to the Anthropic API as Layer 3 when the request is not deflected

## Disconnecting

```bash
isartor connect claude --disconnect
```

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| Claude not routing through Isartor | `settings.json` not updated | Run `isartor connect claude` |
