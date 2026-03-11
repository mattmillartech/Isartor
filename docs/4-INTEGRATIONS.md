# Isartor Integration Guide

Isartor is a drop-in replacement for the OpenAI v1/chat/completions endpoint. It accepts standard OpenAI JSON payloads and returns identical response formats, making integration seamless for any tool or library that supports OpenAI.

## Supported Integrations

### Cursor IDE
- Add your Isartor endpoint as a custom OpenAI provider.
- Endpoint: `http://localhost:8080/v1/chat/completions`
- No API key required for Minimalist Mode.

### VS Code (Continue.dev)
- Configure Continue.dev to use Isartor as the OpenAI endpoint.
- Set the endpoint to `http://localhost:8080/v1/chat/completions`.
- No API key required for Minimalist Mode.

### LangChain (Python)
- Set environment variables to point LangChain to Isartor:

```bash
export OPENAI_API_BASE=http://localhost:8080/v1
export OPENAI_API_KEY=sk-anything
```
- Use LangChain's OpenAI chat completion classes as usual.

## Example: Standard OpenAI Chat Completion Payload

```json
{
  "model": "gemma-2-2b-it",
  "messages": [
    {"role": "user", "content": "Hello, world!"}
  ]
}
```

## Response Format

Isartor returns JSON responses identical to OpenAI:

```json
{
  "id": "chatcmpl-abc123",
  "object": "chat.completion",
  "choices": [
    {
      "message": {
        "role": "assistant",
        "content": "Hello! How can I help you today?"
      }
    }
  ],
  "usage": { ... }
}
```

---

For advanced integration, see [README.md](../README.md) and [docs/2-ARCHITECTURE.md](2-ARCHITECTURE.md).
