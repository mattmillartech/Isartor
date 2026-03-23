# Claude Code + GitHub Copilot

Run Claude Code through Isartor while using your GitHub Copilot subscription as
the Layer 3 backend. Isartor still deflects repeated prompts locally first, so
cache hits consume **zero** Copilot quota.

> **Status:** experimental. The connector and Copilot-backed Layer 3 routing are
> implemented, but Isartor's Anthropic compatibility surface is still
> text-oriented today. Plain Claude prompting works best right now.

## Prerequisites

1. Active GitHub Copilot subscription
2. Isartor running
3. Claude Code installed

```bash
npm install -g @anthropic-ai/claude-code
isartor up --detach
```

## Setup

### Interactive (recommended)

```bash
isartor connect claude-copilot
```

This now prefers **GitHub device-flow OAuth** first. If Isartor already has a
saved OAuth credential, it reuses it. Legacy saved PATs are not auto-reused.

### With an existing token

```bash
isartor connect claude-copilot --github-token ghp_YOUR_TOKEN
```

Use `--github-token` only when you explicitly want to override browser login
with a PAT.

### With custom models

```bash
isartor connect claude-copilot \
  --github-token ghp_YOUR_TOKEN \
  --model gpt-4.1 \
  --fast-model gpt-4o-mini
```

Restart Isartor after the connector writes `./isartor.toml`:

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

## Copilot-backed models

| Model | Type | Notes |
|---|---|---|
| `claude-sonnet-4.5` | Balanced | Default |
| `claude-haiku-4.5` | Fast | Lower latency |
| `gpt-4o` | Strong general model | Good coding default |
| `gpt-4o-mini` | Fast + cheap | Great fast/background model |
| `gpt-4.1` | Included | Safe fallback |
| `o3-mini` | Reasoning | Higher-latency reasoning model |

## What Isartor saves

```text
Without Isartor:
  every Claude Code prompt -> Copilot API -> quota consumed

With Isartor:
  L1a exact hit    -> local cache -> 0 quota
  L1b semantic hit -> local cache -> 0 quota
  cache miss       -> Copilot API -> quota consumed
```

Example:

```text
100 prompts
  40 exact repeats
  25 semantic variants
  35 novel prompts

Only 35 reach Copilot
```

## Limitations

- Copilot output is capped at ~16k tokens
- Advanced Anthropic tool-use flows are not yet fully preserved by Isartor's
  `/v1/messages` compatibility surface
- Extended Anthropic-specific features are not available

## Disconnect

```bash
isartor connect claude-copilot --disconnect
```

## Troubleshooting

- **Authentication failed** → re-run `isartor connect claude-copilot` and complete browser sign-in
- **No active Copilot subscription** → verify the signed-in GitHub user has an active Copilot seat or entitlement
- **Model not found** → retry with `--model gpt-4.1`
- **Still using old provider** → restart Isartor after the config change
