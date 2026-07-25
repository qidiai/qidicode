# Changelog

All notable changes to QIDI Code will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- System prompt identity changed from "Grok released by xAI" to "QIDI Code"
- TUI menu items and status bar localized to bilingual (Chinese + English)
- Provider switched to intern-s1 (InternLM/书生大模型) via OpenAI-compatible API

### Security
- Four rounds of security audit completed (62 findings total: 15 Critical, 15 High, 30 Medium, 5 Low)
- 44 findings fixed across security dimensions including:
  - Ed25519 signing fail-closed
  - Credential Debug redact + zeroize
  - Path boundary checks (LocalFs)
  - Session files 0o600/0o700 permissions
  - Plugin git clone https-only + --no-hooks
  - Bash deny-list (sudo/dd/mkfs/curl|sh)
  - Subagent max_concurrent=10 enforcement
  - MCP bridge input_schema validation
  - Circuit breaker registry key length limits
  - Crash file O_NOFOLLOW + 0o600
  - Clone URL validation (bypass fix)
  - Subagent concurrent limit wired into handle_subagent_request
  - Plan mode tool namespace prefix fix

### Fixed
- Compilation warnings: unreachable pattern, hidden lifetime, unused import, dead code
- Ensure plan mode tools uses correct namespace prefix
- TOML model ID with dots parsed as nested table (config.toml)

### Added
- GitHub Actions CI/CD workflow (ci.yml)
- This CHANGELOG.md

### Known Issues
- 18 remaining Medium/Low security findings (rate limiting, static encryption, HMAC, MCP tool override, hooks trust model, LLM truncation)
- Fork residue: ~269 files still contain `grok_build`/`GrokBuild` references
- ~29K unwrap/expect calls (majority in test code)
- 178 files contain unsafe code (mostly FFI and platform-specific)
- 217 TODO/FIXME markers
- No real test coverage measurement (tarpaulin/llvm-cov not yet run)

## [0.1.220-alpha.4] - 2026-07-22

### Initial Fork
- Forked from grok-build as QIDI Code
- Renamed workspace crates from `xai-grok-*` to `cf-*`
- Native Windows support (no WSL required)
- 72 crates, ~2,140 source files
- Supports OpenAI/Anthropic/xAI providers via chat completions API
- ratatui-based TUI with 32MB stack thread for Windows debug builds
- Config directory: `~/.qidi/`
