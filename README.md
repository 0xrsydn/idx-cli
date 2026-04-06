# idx-cli

CLI tool for Indonesian stock market (IDX) analysis, built in Rust for humans and AI agents.

## Installation

### Cargo

```bash
cargo install idx-cli
```

This installs the `idx` binary. `idx-cli` currently requires Rust `1.85+`.

If you use the Cargo install path directly, some commands also require helper tools at runtime:

- Yahoo-authenticated flows require a `curl_chrome*` binary from `curl-impersonate-chrome` in `PATH`, or `IDX_CURL_IMPERSONATE_BIN` pointing at that binary.
- Ownership PDF import requires `mutool` from MuPDF in `PATH`.

Install those helpers with your OS package manager, or use the Nix app/package below so they are wrapped automatically.

### Nix

```bash
nix run github:0xrsydn/idx-cli -- version
nix profile install github:0xrsydn/idx-cli#default
```

The Nix app wraps `idx` with `curl-impersonate` and `mupdf`, so the Yahoo auth and ownership PDF helper tools are available automatically.

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
