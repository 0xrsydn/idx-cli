# AGENTS.md

`idx-cli` is a Rust CLI (binary `idx`) for Indonesian stock market analysis
(`stocks`) and KSEI shareholder ownership (`ownership`).

## Navigation
```text
crates/idx-core/            library: all data access and analysis
├── src/client.rs           Idx entry point, CachePolicy, Fetched<T>
├── src/{market,financial,investor}.rs   domain APIs over providers and the DB
├── src/api/                providers: msn/ (default), yahoo/ (history/OHLCV)
├── src/analysis/           technical, fundamental, signals
├── src/ownership/          xlsx + pdf parsers, archive (ZIP/TXT), remote discovery,
│                           snapshot sync, db (SQLite/FTS5), entities, search, graph
├── src/{cache,config,error,curl_impersonate}.rs, lib.rs (runtime)
└── tests/fixtures/         shared fixtures (also read by the mock provider)
crates/idx-cli/             binary `idx`
├── src/cli/                clap commands: stocks, ownership, config, cache
├── src/config.rs           IdxConfig: output format and color resolution
├── src/output/             table.rs, json.rs, style.rs
└── tests/                  cli.rs (behavior), golden.rs + golden/ (output parity)
scripts/                    smoke, installer, and ownership snapshot publisher
```

Docs:
- `docs/ARCHITECTURE.md`: providers, ownership ingest/discovery, error and output contract
- `docs/CONVENTIONS.md`: types, naming, testing, and git rules
- `docs/OWNERSHIP_SYNC.md` / `docs/OWNERSHIP_PUBLISH.md`: snapshot consumer contract / maintainer publishing
- `docs/SMOKE.md`: Nix checks, test layout, and the live drift check
- `docs/DESIGN.md`: idx-core / idx-mcp design and parity gates
- `ROADMAP.md`: open work

## Rules
1. Keep data access provider-driven; no ad hoc fetches in `cli/`.
2. Keep parse/normalize functions pure.
3. Tests use fixtures and `IDX_USE_MOCK_PROVIDER=1` (needs idx-core's `testing`
   feature, on under `cargo test`); never hit the live network.
4. Output contract: tables on stdout, one JSON document with `-o json`, errors
   on stderr (JSON-shaped in JSON mode), stdout empty on failure.
5. Keep ownership queries (`releases`, `ticker`, `changes`, ...) working across
   every ingest path: XLSX, legacy PDF, archive ZIP/TXT, and snapshot sync.
6. `ownership import --fetch-bing` is intentionally unsupported.
7. Output parity is a contract: never edit golden files to make a change pass.
   After changing CLI behavior, run the live check and report its summary;
   report IDX `403`s explicitly rather than counting them as passes.

## Verify
```bash
nix flake check -L                       # canonical gate (fmt, clippy, tests, package)
nix develop --command cargo test <name>  # focused loop
scripts/live-smoke.sh --compare <old>    # live drift check, A/B against an older build
```
