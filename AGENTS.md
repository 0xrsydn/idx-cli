# AGENTS.md

`idx-cli` is a Rust CLI (binary `idx`) for Indonesian stock market analysis
(`stocks`) and KSEI shareholder ownership (`ownership`).

## Navigation
```text
src/
├── main.rs, config.rs, cache.rs, error.rs, curl_impersonate.rs
├── cli/        clap commands: stocks, ownership, config, cache
├── api/        providers: msn/ (default), yahoo/ (history/OHLCV)
├── analysis/   technical, fundamental, signals
├── ownership/  xlsx + pdf parsers, archive (ZIP/TXT), remote discovery,
│               snapshot sync, db (SQLite/FTS5), entities, search, graph
└── output/     table.rs, json.rs
tests/cli.rs    CLI integration tests; fixtures in tests/fixtures/
scripts/        smoke, installer, and ownership snapshot publisher
```

Docs:
- `docs/ARCHITECTURE.md`: providers, ownership ingest/discovery, error and output contract
- `docs/CONVENTIONS.md`: types, naming, testing, and git rules
- `docs/OWNERSHIP_SYNC.md` / `docs/OWNERSHIP_PUBLISH.md`: snapshot consumer contract / maintainer publishing
- `docs/SMOKE.md`: smoke runner and Nix check coverage
- `ROADMAP.md`: open work

## Rules
1. Keep data access provider-driven; no ad hoc fetches in `cli/`.
2. Keep parse/normalize functions pure.
3. Tests use fixtures and `IDX_USE_MOCK_PROVIDER=1`; never hit the live network.
4. Output contract: tables on stdout, one JSON document with `-o json`, errors
   on stderr (JSON-shaped in JSON mode), stdout empty on failure.
5. Keep ownership queries (`releases`, `ticker`, `changes`, ...) working across
   every ingest path: XLSX, legacy PDF, archive ZIP/TXT, and snapshot sync.
6. `ownership import --fetch-bing` is intentionally unsupported.

## Verify
```bash
nix flake check -L                       # canonical gate (fmt, clippy, tests, package, smoke)
nix develop --command cargo test <name>  # focused loop
scripts/live-smoke.sh --mode full        # opt-in live providers
```
