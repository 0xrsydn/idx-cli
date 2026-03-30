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
│   ├── cache.rs         # cache info/clear
│   └── ownership.rs     # ownership import/query commands
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

## Current Status
- Automated coverage is healthy: `cargo test` currently passes with 122 tests (86 unit, 36 integration).
- Live `stocks` commands are implemented and smoke-tested for: `quote`, `history`, `technical`, `growth`, `valuation`, `risk`, `fundamental`, `compare`, `profile`, `financials`, `earnings`, `sentiment`, `insights`, `news`, `screen`.
- `stocks history --history-provider msn` is intentionally unsupported for IDX; `auto` falls back to Yahoo.
- `ownership import --fetch-bing` is still intentionally unsupported; Bing client groundwork exists but the CLI path is deferred.

## Known Hardening Gaps
- MSN-only commands still bypass the shared cache/offline path in `src/cli/stocks.rs`; `--offline` is not reliable for `profile`/`financials`/`earnings`/`sentiment`/`insights`/`news`/`screen`.
- Core quote flow has a verified `--offline --no-cache` bug: stale cache can still be served.
- Startup/config failures do not yet honor the JSON error contract; runtime failures do.
- `stocks screen --filter` and `--region` still silently coerce invalid values instead of rejecting them.
- Some live MSN output is incomplete or misleading:
  - `profile` can return sparse fields.
  - `insights.last_updated` is still empty.
  - `financials` table output has malformed negative-number formatting in some rows.

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
4. **Provider abstraction first** — all data access should flow through traits/factories. Note: current MSN-only stock commands still instantiate `MsnProvider` directly in `src/cli/stocks.rs`; removing that split path is an active hardening target.
5. **Sync only** — no tokio/async. CLI tool, ureq is sufficient.
6. **Test with fixtures** — never hit live APIs in tests. Mock provider + fixture JSON.
7. **Output contract** — table to stdout (humans), `--output json` (machines), errors to stderr.
8. **Feature-gated modules** — `ownership` feature for SQLite dep, keeps base binary lean.

## Docs
Start with the repo-visible docs:
- `FEATURE_SPEC.md` — current hardening backlog and CLI truth-pass expectations
- `TODO.md` — working task list, including latest smoke findings
- `docs/ARCHITECTURE.md` — provider/capability design and error strategy
- `docs/CONVENTIONS.md` — repo conventions
