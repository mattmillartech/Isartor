# Claude Code + GitHub Copilot

Use Claude Code's editor and CLI workflow while routing Layer 3 through your
existing GitHub Copilot subscription via Isartor. Repeated prompts are still
deflected by Isartor's L1a/L1b cache first, ad L2 SLM (if turned on) so cache hits consume **zero**
Copilot quota.

> **Current status:** experimental. The connector and Copilot-backed L3 routing
> are implemented, but Isartor's Anthropic compatibility surface is still
> text-oriented today. That means plain Claude Code prompting works best right
> now; more advanced Anthropic tool-use blocks may still require follow-up work.

## Prerequisites

1. Active GitHub Copilot subscription
2. Isartor installed
3. Claude Code installed

```bash
# Install Isartor
curl -fsSL https://raw.githubusercontent.com/isartor-ai/Isartor/main/install.sh | sh

# Install Claude Code
npm install -g @anthropic-ai/claude-code
```

## Setup

### Path A — Interactive authentication (recommended)

```bash
isartor connect claude-copilot
```

This starts GitHub device-flow authentication, stores the OAuth token locally,
updates `./isartor.toml`, and writes Claude Code settings into
`~/.claude/settings.json`.

When no `--github-token` is provided, Isartor now prefers **browser/device-flow
OAuth first**. It will reuse a previously saved OAuth credential, but it will
not silently reuse legacy saved PATs.

### Path B — Use an existing GitHub token

```bash
isartor connect claude-copilot --github-token ghp_YOUR_TOKEN
```

Use `--github-token` only when you intentionally want to override the default
browser login flow with a PAT.

### Path C — Choose custom Copilot models

```bash
isartor connect claude-copilot \
  --github-token ghp_YOUR_TOKEN \
  --model gpt-4.1 \
  --fast-model gpt-4o-mini
```

After the command finishes, restart Isartor so the new Layer 3 config is
loaded:

```bash
isartor stop
isartor up --detach
claude
```

### One-click smoke test

```bash
./scripts/claude-copilot-smoke-test.sh
# or
make smoke-claude-copilot
```

The script automatically:

- reads the saved Copilot credential from `~/.isartor/providers/copilot.json`
- picks a supported Copilot-backed model
- starts a temporary Isartor instance
- runs a Claude Code smoke prompt
- prints an ROI demo showing L3, L1a exact-hit, and L1b semantic-hit behavior

## What the command changes

### `~/.claude/settings.json`

The command writes these Claude Code environment overrides:

| Setting | Value | Purpose |
|---|---|---|
| `ANTHROPIC_BASE_URL` | `http://localhost:8080` (or your gateway URL) | Routes Claude Code to Isartor |
| `ANTHROPIC_AUTH_TOKEN` | `dummy` or your gateway key | Satisfies Claude Code auth requirements |
| `ANTHROPIC_MODEL` | selected model | Primary Copilot-backed model |
| `ANTHROPIC_DEFAULT_SONNET_MODEL` | selected model | Default Claude Code Sonnet mapping |
| `ANTHROPIC_DEFAULT_HAIKU_MODEL` | fast model | Lightweight/background tasks |
| `DISABLE_NON_ESSENTIAL_MODEL_CALLS` | `1` | Reduce unnecessary quota burn |
| `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` | `1` | Compatibility flag across Claude Code versions |
| `ENABLE_TOOL_SEARCH` | `true` | Preserve Claude Code tool search behavior |
| `CLAUDE_CODE_MAX_OUTPUT_TOKENS` | `16000` | Stay under Copilot's output cap |

### `./isartor.toml`

The command also sets Isartor Layer 3 to use the Copilot provider:

```toml
llm_provider = "copilot"
external_llm_model = "claude-sonnet-4.5"
external_llm_api_key = "ghp_..."
external_llm_url = "https://api.githubcopilot.com/chat/completions"
```

## Available Copilot-backed models

| Model | Type | Notes |
|---|---|---|
| `claude-sonnet-4.5` | Balanced | Good default for Claude-style behavior |
| `claude-haiku-4.5` | Fast | Lower-latency Claude-family option |
| `gpt-4o` | Strong general model | Good for broad coding tasks |
| `gpt-4o-mini` | Fast + cheap | Good default fast/background model |
| `gpt-4.1` | Included | Safe fallback choice |
| `o3-mini` | Reasoning | Higher-latency reasoning model |

## What Isartor saves

Without Isartor:

```text
Every Claude Code prompt -> GitHub Copilot API -> quota consumed
```

With Isartor:

```text
Repeated prompt (L1a hit) -> served locally -> 0 Copilot quota
Similar prompt (L1b hit)  -> served locally -> 0 Copilot quota
Novel prompt (cache miss) -> forwarded to Copilot -> quota consumed
```

Example session:

```text
100 Claude Code prompts
  40 exact repeats      -> L1a -> 0 quota
  25 semantic variants  -> L1b -> 0 quota
  35 novel prompts      -> L3  -> 35 Copilot-backed requests

Result: 35 routed requests instead of 100
```

## Limitations

- GitHub Copilot output is capped; Isartor writes `CLAUDE_CODE_MAX_OUTPUT_TOKENS=16000`
- The current `/v1/messages` compatibility path is still text-oriented, so some
  advanced Anthropic tool-use flows may not yet behave exactly like direct
  Anthropic routing
- Extended-thinking / provider-specific Anthropic features are not preserved
- If the chosen Copilot model is unavailable to your account, requests fail
  instead of silently falling back to Anthropic

## Disconnect

```bash
isartor connect claude-copilot --disconnect
```

This restores the backed-up `~/.claude/settings.json` and `./isartor.toml`.

## Troubleshooting

| Error | Cause | Fix |
|---|---|---|
| `Authentication failed` | Browser login incomplete, token invalid, or expired | Re-run `isartor connect claude-copilot` and finish GitHub sign-in |
| `No active GitHub Copilot subscription` | Signed-in GitHub user has no active Copilot seat / entitlement | Check `https://github.com/features/copilot` and enterprise seat assignment |
| `Model not found` | Account cannot access the requested model | Retry with `--model gpt-4.1` |
| `Claude Code still uses Anthropic` | Isartor not restarted after config change | Run `isartor stop && isartor up --detach` |
| `401` from Isartor | Gateway auth enabled but Claude settings use dummy token | Re-run with the gateway key available in local config |
| `Tool call failed` | Current Anthropic compatibility is still text-first | Use simpler prompting for now; full tool-use compatibility is follow-up work |
