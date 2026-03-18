# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

If you discover a security vulnerability in Isartor, please report it responsibly.

**Do NOT open a public GitHub issue for security vulnerabilities.**

Instead, please email **security@isartor.ai** with:

1. A description of the vulnerability
2. Steps to reproduce or a proof-of-concept
3. The version(s) affected

We will acknowledge your report within **48 hours** and aim to provide a fix or mitigation within **7 days** for critical issues.

## Scope

The following are in scope:

- The `isartor` binary and its HTTP gateway
- The CONNECT proxy (TLS MITM) functionality
- Configuration file parsing and secret handling
- Docker images published to `ghcr.io/isartor-ai/isartor`

## Security Best Practices

- Never commit API keys or secrets to the repository
- Use `isartor set-key` or `*_FILE` environment variables for secret management
- Run Isartor behind a reverse proxy in production
- Keep your installation up to date
