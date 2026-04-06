# AGENTS.md

## Project
`idx-cli` is a Rust CLI for Indonesian stock market analysis and ownership workflows.

The repo currently has two main product areas:
- `stocks`: live market data and analysis
- `ownership`: import/sync once, then query locally from SQLite

## Stack
- Rust stable
- `clap` 4 for CLI
- `ureq` 3 for HTTP
- `comfy-table` and `owo-colors` for output
- `rusqlite` + bundled SQLite/FTS5 for ownership
- TOML config in `~/.config/idx/config.toml`
- file cache in `~/.cache/idx/`

## Source Map
```text
src/
├── main.rs
├── cli/
│   ├── stocks.rs
│   ├── ownership.rs
│   ├── config.rs
│   └── cache.rs
├── api/
│   ├── mod.rs
│   ├── types.rs
│   ├── yahoo/
│   └── msn/
├── analysis/
├── ownership/
│   ├── archive.rs
│   ├── db.rs
│   ├── entities.rs
│   ├── parser.rs
│   ├── remote.rs
│   ├── snapshot.rs
│   └── types.rs
├── output/
├── cache.rs
├── config.rs
└── error.rs
```

## Provider Model
- `MSN` is the primary/default provider for quotes, fundamentals, profile, earnings, financials, sentiment, insights, news, and screener data.
- `Yahoo` is the fallback provider for history/OHLCV because MSN history for IDX is still not supported.
- Current config knobs:
  - `IDX_PROVIDER=msn|yahoo`
  - `IDX_HISTORY_PROVIDER=auto|yahoo|msn`

## Ownership Model
Ownership is local-first after bootstrap.

Preferred bootstrap/update path:
1. `idx ownership sync`
2. if no snapshot manifest is available: `idx ownership discover` then `idx ownership import --url <pdf-url>`
3. local `--file` imports remain available for manual/fallback use

Ownership input paths:
- primary remote source: discoverable IDX `above1` holder-register PDF
- maintained snapshot path: `ownership sync`
- local fallback path: PDF, plus local archive `.zip` / `.txt`

Important scope note:
- archive ZIP/TXT ingest is a fallback/backstop path, not the primary product ingest surface
- `ownership import --fetch-bing` is still intentionally unsupported

## Working Principles
1. Keep data access provider-driven where possible; avoid adding new ad hoc fetch paths at the CLI layer.
2. Prefer pure parse/normalize transforms over hidden state.
3. Use fixtures in tests; do not hit live network in automated tests.
4. Preserve the output contract: table to stdout, JSON with `--output json`, errors to stderr.
5. Treat ownership schema/query compatibility as important: `releases`, `ticker`, and `changes` should keep working across ingest paths.

## Verification
Core verification:
```bash
nix develop
cargo build
cargo clippy -- -D warnings
cargo test
```

Smoke tooling:
```bash
scripts/live-smoke.sh
scripts/live-smoke.sh --mode mock
scripts/live-smoke.sh --mode full
```

## Read First
Start with these repo docs before making changes:
- `FEATURE_SPEC.md` — active implementation backlog and remaining core gaps
- `TODO.md` — execution tracker and smoke notes
- `docs/ARCHITECTURE.md` — provider and ownership flow
- `docs/OWNERSHIP_SYNC.md` — snapshot sync contract
- `docs/SMOKE.md` — reusable smoke commands
- `docs/CONVENTIONS.md` — repo conventions
