# Changelog

All notable changes to Isartor will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Windows x86_64 build target in release pipeline
- SECURITY.md, CODE_OF_CONDUCT.md, CODEOWNERS
- Dependabot for Cargo, Actions, and Docker updates
- Dynamic version badge from GitHub Releases

### Changed
- All install URLs now point to `isartor-ai/Isartor` (no more `isartor-dist` indirection)
- Consolidated architecture docs into single `docs/2-ARCHITECTURE.md`
- Cleaned release notes template (removed private-repo install paths)

### Removed
- `dist_release.yml` workflow (isartor-dist no longer needed)
- Duplicate `scripts/install.sh` and `scripts/install.ps1`
- Private-repo install instructions from README and docs
- `azure_openai_key/` directory
- Stale `architecture.md` and `docs/ARCHITECTURE.md` duplicates

## [0.1.18] - 2025-07-14

### Added
- HTTP CONNECT proxy with TLS MITM for GitHub Copilot CLI (`isartor connect copilot`)
- Proxy listens on `:8081` by default (`ISARTOR__PROXY_PORT`)
- Auto-generated CA stored at `~/.isartor/ca/`
- Per-host server certificates via rcgen
- Domain allowlist for transparent tunneling

## [0.1.17] - 2025-07-14

### Added
- `isartor set-key` CLI subcommand for configuring LLM provider API keys
- Supports OpenAI, Azure, Anthropic, Groq, Mistral providers
- Interactive secure input via rpassword
- TOML config merge via toml_edit
- `--env-file` mode for Docker secret workflows
- `--dry-run` flag for previewing changes

## [0.1.16] - 2025-07-13

### Added
- Publish releases to distribution repo via `dist_release.yml`

### Fixed
- Linux musl builds now use cargo-zigbuild for static linking

## [0.1.15] - 2025-07-13

### Fixed
- Build musl targets with runner-based zigbuild (no Docker container)

[Unreleased]: https://github.com/isartor-ai/Isartor/compare/v0.1.18...HEAD
[0.1.18]: https://github.com/isartor-ai/Isartor/compare/v0.1.17...v0.1.18
[0.1.17]: https://github.com/isartor-ai/Isartor/compare/v0.1.16...v0.1.17
[0.1.16]: https://github.com/isartor-ai/Isartor/compare/v0.1.15...v0.1.16
[0.1.15]: https://github.com/isartor-ai/Isartor/releases/tag/v0.1.15
