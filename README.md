# idx-cli

CLI tool for Indonesian stock market (IDX) analysis, built in Rust for humans and AI agents.

## Installation

```bash
cargo install idx-cli
```

```bash
nix run github:0xrsydn/idx-cli
```

## Quick start

```bash
idx stocks quote BBCA
idx stocks quote BBCA,BBRI,BMRI
idx -o json stocks quote BBCA
idx stocks history BBCA --period 3mo
idx config init
idx cache info
idx --offline stocks quote BBCA
```

## Features

- Provider abstraction (swap data sources behind one interface)
- Local file cache with TTL
- Offline mode with stale-cache fallback
- Human table output and machine-friendly JSON output

## Configuration

Config file location:

- `~/.config/idx/config.toml`

You can configure using:

- Config file (`idx config init`, `idx config set`)
- Environment variables
- CLI flags

Precedence order:

1. CLI flags
2. Environment variables
3. Config file
4. Built-in defaults

## Agent-friendly usage

- Use `idx --help` and subcommand help for discoverability
- Use `-o json` / `--output json` for structured output
- See `skills/` for agent workflows and task recipes

## Development

```bash
nix develop
cargo test
```

Prek hooks are configured for quality gates:

- pre-commit: `cargo fmt --check` + `cargo clippy -- -D warnings`
- pre-push: `cargo test`

## License

MIT
