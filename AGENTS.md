# AGENTS.md

`idx-cli` is a Rust CLI (binary `idx`) for Indonesian stock market analysis
(`stocks`) and KSEI shareholder ownership (`ownership`), plus an MCP server
(binary `idx-mcp`) over the same library.

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
crates/idx-mcp/             binary `idx-mcp`: stdio MCP tools (server.rs), inputs (params.rs)
└── tests/mcp.rs            JSON-RPC over stdio against the mock provider
tests/agents/               opt-in agent smoke test: real coding agents against idx-mcp
web/                        website + user docs (Astro Starlight, bun); deployed by Cloudflare Pages
scripts/                    smoke, installer, and ownership snapshot publisher
```

Docs:
- `docs/ARCHITECTURE.md`: providers, ownership ingest/discovery, error and output contract
- `docs/CONVENTIONS.md`: types, naming, testing, and git rules
- `docs/OWNERSHIP_SYNC.md` / `docs/OWNERSHIP_PUBLISH.md`: snapshot consumer contract / maintainer publishing
- `docs/SMOKE.md`: Nix checks, test layout, and the live drift check
- `docs/DESIGN.md`: idx-core / idx-mcp design and parity gates
- `docs/IDX_NEWS.md`: news sources research (IDX disclosures, Google News, Yahoo) and a proposed publisher design
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
tests/agents/agent-smoke.sh              # after changing idx-mcp tools: real agents, opt-in
nix develop .#web --command sh -c 'cd web && bun install && bun run build'   # website
```

## Website
User-facing docs live in `web/src/content/docs/`; `docs/` is for maintainers.
When CLI help or idx-mcp tools change, regenerate the reference pages
(`web/scripts/gen-cli-reference.sh <idx>` and `web/scripts/gen-mcp-tools.sh
crates/idx-mcp/tests/golden/tools.json`); `nix flake check` fails until you do.
