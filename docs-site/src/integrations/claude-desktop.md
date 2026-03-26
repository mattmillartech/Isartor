# Claude Desktop

Claude Desktop integrates with Isartor via a local MCP server. The recommended setup is `isartor connect claude-desktop`, which registers `isartor mcp` in Claude Desktop's config so Claude can use Isartor's cache-aware tools.

## Step-by-step setup

```bash
# 1. Start Isartor
isartor up --detach

# 2. Register Isartor in Claude Desktop
isartor connect claude-desktop

# 3. Restart Claude Desktop
```

After restart, open Claude Desktop's tools/connectors UI and confirm the `isartor` MCP server is present.

## What the connector writes

`isartor connect claude-desktop` updates Claude Desktop's local MCP config and keeps a backup next to it.

Typical config paths:

- macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`
- Windows: `%APPDATA%\Claude\claude_desktop_config.json`
- Linux (best-effort path): `~/.config/Claude/claude_desktop_config.json`

The generated MCP entry looks like:

```json
{
  "mcpServers": {
    "isartor": {
      "command": "/path/to/isartor",
      "args": ["mcp"],
      "env": {
        "ISARTOR_GATEWAY_URL": "http://localhost:8080"
      }
    }
  }
}
```

If gateway auth is enabled, the connector also writes `ISARTOR__GATEWAY_API_KEY` into the managed server env block.

## What Claude Desktop gets

The Isartor MCP server exposes these tools:

- `isartor_chat` — cache-first lookup through Isartor's L1a/L1b layers
- `isartor_cache_store` — store prompt/response pairs back into Isartor after a cache miss

This gives Claude Desktop a low-risk integration path that fits the current MCP model without relying on Anthropic base-URL overrides.

## Advanced / manual setup

If you prefer to edit the config yourself, add a local MCP server entry that runs:

```bash
isartor mcp
```

Isartor also exposes MCP over HTTP/SSE at:

```text
http://localhost:8080/mcp/
```

That remote MCP surface is useful for clients that support HTTP/SSE registration directly, but `isartor connect claude-desktop` currently uses the local stdio flow because it is the most reliable Claude Desktop path today.

## Disconnecting

```bash
isartor connect claude-desktop --disconnect
```

This restores the backup when one exists; otherwise it removes only the managed `mcpServers.isartor` entry.

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| Claude Desktop shows no `isartor` tools | Claude Desktop was not restarted | Quit and relaunch Claude Desktop after `isartor connect claude-desktop` |
| Tools appear but calls fail | Isartor is not running | Start the gateway with `isartor up --detach` |
| MCP server is present but unauthorized | Gateway auth enabled | Re-run `isartor connect claude-desktop --gateway-api-key <key>` |
| You want the original config back | Managed config needs rollback | Run `isartor connect claude-desktop --disconnect` |

## Note on desktop extensions

Claude Desktop now supports desktop extensions, but Isartor's first-class integration in this repo uses the simpler local MCP server flow today. That keeps setup light and works with the existing `isartor mcp` implementation immediately.
