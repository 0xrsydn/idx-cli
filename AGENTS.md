# AGENTS.md

## Project
`idx-cli` — CLI tool for Indonesian stock market (IDX) analysis. Built in Rust for humans and AI agents. Single binary, zero runtime deps.

## Stack
- **Language:** Rust (stable, via rust-overlay)
- **CLI:** clap 4 (derive)
- **HTTP:** ureq 3 (sync, no async runtime)
- **Output:** comfy-table, owo-colors
- **Config:** TOML (`~/.config/idx/config.toml`)
- **Cache:** JSON file-based (`~/.cache/idx/`)
- **Testing:** cargo nextest, assert_cmd, predicates
- **Hooks:** prek (pre-commit: fmt+clippy, pre-push: test)

## Structure
```
src/
├── main.rs              # Entry point, clap setup, command dispatch
├── cli/                 # Command definitions (clap structs + handlers)
│   ├── stocks.rs        # stocks quote, history commands
│   ├── config.rs        # config get/set/init/path
│   └── cache.rs         # cache info/clear
├── api/                 # Data provider abstraction + implementations
│   ├── mod.rs           # MarketDataProvider trait
│   ├── yahoo.rs         # Yahoo Finance provider (query2 endpoint)
│   └── types.rs         # Quote, OHLC, Period, Interval types
├── output/              # Rendering layer (table, json)
│   ├── table.rs         # comfy-table + owo-colors
│   └── json.rs          # serde_json pretty print
├── cache.rs             # File-based TTL cache
├── config.rs            # Config loading + merge (flags > env > file > defaults)
└── error.rs             # IdxError enum (thiserror)
tests/
├── cli.rs               # Integration tests (assert_cmd, mock provider)
docs-internal/           # (gitignored) Specs, research, business strategy
```

## Development
```bash
# Enter dev shell (requires Nix + direnv)
direnv allow  # or: nix develop

# Build
cargo build

# Run
cargo run -- stocks quote BBCA
cargo run -- -o json stocks quote BBCA,BBRI
cargo run -- stocks history BBCA --period 3mo

# Test
cargo nextest run   # or: cargo test

# Lint
cargo fmt --check
cargo clippy -- -D warnings
```

## Docs
- `docs-internal/SPEC.md` — system design, command tree, milestones (gitignored)
- `docs-internal/TODO.md` — task breakdown with checklist (gitignored)
- `docs-internal/OWNERSHIP_FEATURE_DESIGN.md` — ownership intelligence feature design (gitignored)

## Verification
```bash
cargo build              # must compile
cargo clippy -- -D warnings  # zero warnings
cargo nextest run        # all tests pass
```
Hooks enforce this: prek runs fmt+clippy on commit, tests on push.

## Rules
1. **Provider abstraction** — all data access goes through `MarketDataProvider` trait, never call Yahoo directly from commands
2. **Sync only** — no tokio/async, this is a CLI tool using ureq
3. **Test with fixtures** — never hit live APIs in tests, use mock provider + fixture JSON
4. **Output contract** — table mode to stdout for humans, `--json` for machines, errors to stderr
5. **Symbol resolution** — always normalize symbols (`BBCA` → `BBCA.JK`) before API calls
