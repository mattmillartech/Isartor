# Gemini CLI

Gemini CLI integrates via `GEMINI_API_BASE_URL`, routing requests through
Isartor's gateway.

## Step-by-step setup

```bash
# 1. Start Isartor
isartor up

# 2. Configure Gemini CLI
isartor connect gemini

# 3. Source the env file
source ~/.isartor/env/gemini.sh

# 4. Run Gemini CLI
gemini
```

## How it works

1. `isartor connect gemini` writes `GEMINI_API_BASE_URL` and `GEMINI_API_KEY` to `~/.isartor/env/gemini.sh`
2. Gemini CLI sends requests to Isartor's gateway
3. Isartor forwards to the configured upstream as Layer 3 when not deflected

## Disconnecting

```bash
isartor connect gemini --disconnect
```

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| Gemini not routing through Isartor | Env vars not loaded | Run `source ~/.isartor/env/gemini.sh` in your shell |
