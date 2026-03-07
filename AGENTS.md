# AGENTS.md

## Project
`idx-cli` — CLI tool for Indonesian stock market (IDX) analysis. Built in Rust for humans and AI agents. Single binary, schema-driven, functional architecture.

## Stack
- **Language:** Rust (stable, via rust-overlay)
- **CLI:** clap 4 (derive)
- **HTTP:** ureq 3 (sync, no async runtime)
- **Output:** comfy-table, owo-colors
- **DB:** rusqlite (bundled SQLite, FTS5) — ownership module
- **Config:** TOML (`~/.config/idx/config.toml`)
- **Cache:** JSON file-based (`~/.cache/idx/`)
- **Testing:** cargo test, assert_cmd, predicates
- **Hooks:** prek (pre-commit: fmt+clippy, pre-push: test)
- **VCS:** jj (Jujutsu, colocated with git)

## Structure
```
src/
├── main.rs              # Entry point, command dispatch
├── cli/                 # Command handlers (clap derive structs)
│   ├── stocks.rs        # stocks quote/history/technical/fundamental/...
│   ├── config.rs        # config get/set/init/path
│   └── cache.rs         # cache info/clear
├── api/                 # Data providers (trait-based abstraction)
│   ├── mod.rs           # MarketDataProvider trait + factory functions
│   ├── types.rs         # All domain types (Quote, Ohlc, Fundamentals, ...)
│   ├── yahoo/           # Yahoo Finance provider (history/OHLCV)
│   └── msn/             # MSN Finance provider (quotes, fundamentals, ++)
├── analysis/            # Technical & fundamental analysis (pure functions)
├── ownership/           # Ownership intelligence module (SQLite-backed)
│   ├── types.rs         # Ownership domain types
│   └── db.rs            # Schema, migrations, queries
├── output/              # Rendering (table, json)
├── cache.rs             # File-based TTL cache
├── config.rs            # Config loading (flags > env > file > defaults)
└── error.rs             # IdxError enum (thiserror)
```

## Providers
- **MSN** = default provider (quotes, fundamentals, profile, earnings, financials, sentiment, insights, news, screener)
- **Yahoo** = automatic fallback for history/OHLCV (MSN doesn't support IDX history)
- Configurable: `IDX_PROVIDER=msn|yahoo`, `IDX_HISTORY_PROVIDER=auto|yahoo|msn`

## Development
```bash
nix develop                                    # enter dev shell
cargo build                                    # build
cargo run -- stocks quote BBCA                 # run
cargo run -- -o json stocks history BBCA       # JSON output
cargo test                                     # test
cargo fmt --check && cargo clippy -- -D warnings  # lint
```

## Verification
```bash
cargo build              # must compile
cargo clippy -- -D warnings  # zero warnings
cargo test               # all tests pass
```

## Principles
1. **Schema-driven** — define types first, build logic around them. Types are the spec.
2. **Functional approach** — pure parse/transform functions (`parse_*`, `normalize_*`), no hidden state.
3. **Data types heavy** — rich enums, newtypes, composite structs. Precision via integer representations (basis points for %, i64 for shares).
4. **Provider abstraction** — all data access through traits, never call Yahoo/MSN directly from commands.
5. **Sync only** — no tokio/async. CLI tool, ureq is sufficient.
6. **Test with fixtures** — never hit live APIs in tests. Mock provider + fixture JSON.
7. **Output contract** — table to stdout (humans), `--output json` (machines), errors to stderr.
8. **Feature-gated modules** — `ownership` feature for SQLite dep, keeps base binary lean.

## Docs
Detailed specs live in `docs-internal/` (gitignored — internal strategy):
- `docs-internal/SPEC.md` — system design, command tree, milestones
- `docs-internal/TODO.md` — sprint breakdown
- `docs-internal/ownership/SPEC.md` — ownership module architecture
- `docs-internal/ownership/TODO.md` — ownership sprint plan
