# System Design: idx-core and its frontends

**Status:** Accepted 2026-10-14. Implementation on `refactor/idx-core`; phases 0-2 done.

## Goal

Turn idx-cli's data layer into `idx-core`, a library that is the single way this
project retrieves Indonesian market, financial, and investor data. The CLI, an
MCP server, and later an HTTP API become thin frontends over it.

Non-goals for now: new data sources, a hosted service, async rewrites of the
provider clients.

## Architecture

```text
 Upstreams                 idx-core (library)                     Frontends
┌──────────────┐   ┌──────────────────────────────────────┐   ┌──────────────┐
│ MSN Finance  │──▶│ sources/    HTTP clients + parsers    │   │ idx-cli      │
│ Yahoo        │──▶│ cache/      TTL, offline, stale       │──▶│  clap, table │
│ IDX / KSEI   │──▶│ store/      ownership SQLite          │   ├──────────────┤
└──────────────┘   │ market/     quotes, history, screen   │──▶│ idx-mcp      │
                   │ financial/  fundamentals, reports     │   │  MCP tools   │
                   │ investor/   ownership queries + sync  │   ├──────────────┤
                   │ analysis/   technical, fundamental    │──▶│ idx-api      │
                   └──────────────────────────────────────┘   │  (later)     │
                                                              └──────────────┘
```

Rule: frontends parse input and render output. Every fetch, cache decision,
filter, sort, and calculation lives in `idx-core`.

## Data Domains

| Domain | Data | Source today | Status |
| --- | --- | --- | --- |
| Market | quotes, OHLCV history, screener | MSN (default), Yahoo (OHLCV) | shipped |
| Market | IHSG index, breadth, movers, sectors | not chosen yet | `ROADMAP.md` |
| Financial | key ratios, statements, earnings, profile | MSN | shipped (single-period statements) |
| Financial | sentiment, insights, news | MSN | shipped |
| Investor | KSEI holders above 1%, entity holdings, changes, concentration, cross-holders, graph | IDX XLSX (legacy PDF), synced snapshot | shipped |
| Investor | institutional holders (Bing) | schema only | ingest unsupported |
| Analysis | technical signals, growth/valuation/risk | derived locally | shipped |

Two access patterns, and the core API should keep them visibly separate:

- **Live data** (market, financial): fetched from upstreams, cached on disk by TTL.
- **Local data** (investor): read from a local SQLite database that is
  populated by `sync` or `import`. Queries never touch the network.

All upstreams are unofficial or scraped. They change without notice, rate limit
datacenter IPs (Yahoo `429`), and sit behind Cloudflare (IDX `403`). The core
must report which source and how fresh each result is, and must never present
stale or fallback data as current.

## idx-core API

A single entry point holds configuration, providers, cache, and the DB path.
Domain methods hang off it.

```rust
let idx = Idx::builder()
    .config(CoreConfig::load()?)   // provider, TTLs, paths; no output settings
    .build()?;

// market
let quotes: Fetched<Vec<Quote>> = idx.market().quotes(&["BBCA", "BBRI"], CachePolicy::Default)?;
let candles: Fetched<Vec<Ohlc>> = idx.market().history("BBCA", Period::M3, Interval::D1, CachePolicy::Default)?;
let screen: Fetched<Vec<Quote>> = idx.market().screen(ScreenQuery { filter, region, limit })?;

// financial
let report: Fetched<FinancialStatements> = idx.financial().statements("BBCA", StatementFilter::all())?;
let earnings: Fetched<EarningsReport> = idx.financial().earnings("BBCA", EarningsFilter::default())?;

// analysis (fetches its own inputs through the same cache)
let tech: Fetched<TechnicalReport> = idx.analysis().technical("BBCA")?;

// investor (local DB)
let holders: TickerOwnership = idx.investor().ticker("BBCA")?;
let status: OwnershipStatus = idx.investor().status()?;      // releases, latest as-of, empty?
let sync: OwnershipSyncResult = idx.investor().sync(SyncOptions::default())?;
```

Key decisions:

- **Blocking API.** `ureq` and `rusqlite` are blocking, and so is the CLI.
  Async frontends (MCP, HTTP) call core through `spawn_blocking`. Revisit only
  if an HTTP API needs high concurrency.
- **`Fetched<T>` envelope** for live data: `{ data, source, fetched_at, stale }`.
  This replaces today's stderr-only stale-cache warning, which an agent or API
  client cannot see.
- **`CachePolicy`**: `Default | Offline | Refresh` replaces the `--offline` /
  `--no-cache` flag handling now spread through `cli/stocks.rs`.
- **Explicit configuration.** Core reads no environment variables. `IDX_*` env
  parsing, `config.toml` loading, and XDG resolution move to a `CoreConfig::load()`
  helper that frontends call; tests construct `CoreConfig` directly.
- **Mock provider behind a `testing` feature.** Today `MockProvider` ships in the
  release binary and reads fixtures relative to the working directory.
- **Typed inputs.** Screener filters, regions, statement and earnings filters
  become enums with `FromStr`, so validation is identical across frontends.

## What Moves Out of `crates/idx-cli/src/cli/`

| From | To |
| --- | --- |
| `fetch_with_cache`, `cache_bucket`, offline/stale handling (`cli/stocks.rs`) | `idx-core` cache layer, behind `CachePolicy` |
| `build_technical_report`, `fetch_fundamental_analysis_report` | `analysis` |
| `filter_financial_statements`, `filter_earnings_report` | `financial` |
| `screener_filter_key`, `screener_region_key`, `sort_screener_quotes`, full-list fetch | `market::screen` |
| ownership command orchestration (`cli/ownership.rs`) | `investor` |
| `IdxConfig.output`, `no_color` | stay in `idx-cli` |

After the move, `cli/stocks.rs` and `cli/ownership.rs` should be argument
parsing plus a call into core plus rendering.

## Error Model

`IdxError` and its stable codes (`SYMBOLNOTFOUND`, `NOMARKETDATA`, `RATELIMITED`,
`UNSUPPORTED`, `INVALIDINPUT`, `OFFLINE`, `CACHEMISS`, `HTTP`, ...) live in core.
Each frontend maps them once:

| Frontend | Mapping |
| --- | --- |
| CLI | unchanged: stderr, JSON `{error, code, message}` in JSON mode, non-zero exit |
| MCP | tool result with `isError: true` and the same `{code, message}` body |
| HTTP | status by class (`INVALIDINPUT` 400, `SYMBOLNOTFOUND` 404, `RATELIMITED` 429, upstream failures 502) plus the same body |

## Agent-Native CLI Output

Agents mostly drive the CLI through a shell and compose it with `jq`, `grep`,
`sed`, Python, and other commands. The CLI should be a well-behaved Unix filter
first and a pretty terminal tool second.

### Output resolution

`-o` flag > `IDX_OUTPUT` > `config.toml` > **auto**. Auto picks:

- stdout is a terminal: `table`
- stdout is a pipe, file, or captured by a harness: `json`

idx cannot tell whether an agent is calling it, only whether stdout is a
terminal (`std::io::IsTerminal`). Agent harnesses capture output, so they get
JSON with no flag. Two edge cases, both handled by the explicit overrides:

- a human piping to `less` or `grep` gets JSON; use `-o table`
- a harness that runs commands inside a pseudo-terminal gets tables; set
  `IDX_OUTPUT=json` in its environment

### Formats

| Format | Shape | Use |
| --- | --- | --- |
| `table` | human-readable | terminals |
| `json` | one document; pretty on a terminal, compact otherwise | default for pipes; `jq`, `json.load` |
| `jsonl` | list results: one record per line; single objects: one line | `head`, `grep`, streaming, `pandas.read_json(..., lines=True)` |

Measured on fixtures, table output is 2-5x the bytes of JSON, and compact JSON
is another 20-30% smaller than pretty JSON.

### Composability contract

- stdout carries only data; diagnostics and warnings go to stderr. Already true.
- Errors in `json`/`jsonl` mode are one JSON object on stderr with stdout empty.
  Already true.
- No ANSI color when stdout is not a terminal. Today some table headers emit
  bold codes even when piped.
- `stocks quote` and `stocks compare` accept `-` to read symbols from stdin,
  separated by whitespace or commas:
  `idx stocks screen --filter low-pe | jq -r '.[].symbol' | idx stocks compare -`
- Exit codes by error class (today every error exits `1`):

  | Exit | Meaning | Codes |
  | --- | --- | --- |
  | 0 | success, including partial `compare` results | |
  | 1 | other failure | `IO`, `DATABASEERROR`, `PARSEERROR`, ... |
  | 2 | invalid usage or input | clap errors, `INVALIDINPUT`, `CONFIGERROR` |
  | 3 | not found or no data | `SYMBOLNOTFOUND`, `NOMARKETDATA`, `CACHEMISS` |
  | 4 | upstream unavailable, retry later | `HTTP`, `RATELIMITED`, `PROVIDERUNAVAILABLE`, `OFFLINE`, `AUTHERROR` |
  | 5 | unsupported | `UNSUPPORTED` |

- JSON keeps today's top-level shape: no `{data, meta}` wrapper, so `jq` paths
  stay short and JSONL works per record. Freshness metadata (`Fetched<T>`) is
  exposed by MCP and the API; the CLI keeps its stderr stale-cache warning.
- Field names, units (basis points, whole IDR, WIB timestamps), and nullability
  are part of the contract and documented in `docs/ARCHITECTURE.md`.

Examples:

```bash
idx stocks quote BBCA BBRI | jq -r '.[] | "\(.symbol) \(.price)"'
idx -o jsonl stocks history BBCA --period 1y | tail -5
idx ownership ticker BBCA | jq '.holders | map(select(.percentage_bps > 500))'
idx -o jsonl stocks screen --filter top-performers \
  | python -c 'import sys, json; [print(json.loads(l)["symbol"]) for l in sys.stdin]'
```

This is a breaking change for anyone parsing table output from a pipe. Ship it
in a minor version with a release note; `general.output = "table"` restores the
old default.

## Frontend: idx-mcp

Stdio transport first, using the `rmcp` crate. Read-only by default.

Tools are grouped by question rather than mirroring every CLI command, which
keeps the tool list short for agents:

| Tool | Inputs | Backed by |
| --- | --- | --- |
| `get_quotes` | `symbols[]` | `market().quotes` |
| `get_price_history` | `symbol`, `period`, `interval` | `market().history` |
| `screen_stocks` | `filter`, `region`, `limit` | `market().screen` |
| `get_company` | `symbol`, `sections[]` (profile, financials, earnings, sentiment, insights, news) | `financial()` |
| `analyze_stock` | `symbol`, `kinds[]` (technical, growth, valuation, risk) | `analysis()` |
| `compare_stocks` | `symbols[]` | `analysis()` |
| `get_shareholders` | `ticker` | `investor().ticker` |
| `get_investor_holdings` | `entity` | `investor().entity` |
| `search_investors` | `query` | `investor().search` |
| `get_ownership_changes` | `from`, `to`, `ticker?` | `investor().changes` |
| `ownership_status` | none | `investor().status` |

Ownership bootstrap: if the local DB is empty, investor tools return an error
telling the agent to call `ownership_sync`. That tool is opt-in through a
server flag, because it downloads several MB.

## Frontend: idx-api (later)

An HTTP server over the same core, e.g. `GET /v1/stocks/{symbol}/quote`. Before
building it, decide whether it is self-hosted only. Publicly re-serving MSN,
Yahoo, or IDX data raises terms-of-use and rate-limit questions that a local CLI
does not.

## Packaging

```text
Cargo workspace (this repo)
├── crates/idx-core   library, published to crates.io
├── crates/idx-cli    binary `idx`  (package name unchanged)
├── crates/idx-mcp    binary `idx-mcp`
npm/
├── idx-cli           existing wrapper
└── idx-mcp           same postinstall download + SHA256SUMS
```

- One version for every crate and npm package, released from one `vX.Y.Z` tag.
  Release assets add `idx-mcp-<platform>` next to the existing `idx-<platform>`.
- Nix: `packages.default` (idx) and `packages.idx-mcp`; both run through
  `nix flake check`.
- The names `idx-core`, `idx-mcp`, and `idx-api` were free on crates.io and npm
  on 2026-10-14.
- Bundling later is a packaging choice: an `idx mcp` subcommand behind a Cargo
  feature, or an npm package that installs both binaries.

## Parity and Verification

The refactor must keep every shipped command's output identical. Two gates
enforce that, and neither may be skipped or weakened to make a change pass.

### Golden output tests (offline, in `nix flake check`)

Before any refactor code lands, capture the current CLI as golden files:

- every `stocks`, `ownership`, `config`, `cache`, `version`, and `completions`
  command, in both `table` and `json`, against the mock provider fixtures and a
  fixture-built ownership DB
- the documented error cases (invalid input, unsupported provider, offline
  without cache, empty ownership DB)
- cases listed in `tests/golden/cases.toml`; each result stored as
  `tests/golden/<case>.<format>.golden` (exit code, stdout, stderr) and
  compared byte for byte by one data-driven test in `tests/golden.rs`

Regenerating goldens (`UPDATE_GOLDEN=1`) is only allowed for an intentional,
reviewed output change, and the diff must be in the same commit with the reason
in the message. Phases 2-4 must not change any golden file. Phase 1 changes only
what it documents: default format resolution, JSON whitespace when piped, ANSI
color when piped (table output emits bold headers into pipes today), and exit
codes; JSON content and table text stay identical.

`tests/cli.rs` stays the behavioral suite and must keep passing unchanged.

### Live A/B check (opt-in, before merging the refactor)

Fixtures cannot show that the new code still talks to real upstreams correctly.
`scripts/live-smoke.sh --compare <old-idx>` runs every command with the
pre-refactor binary and the new binary back to back against live MSN, Yahoo, and
IDX, then compares:

- exit codes and JSON structure (keys and types) for every command
- values for slow-moving data (profile, financials, earnings, ownership)
- volatile fields (price, change, volume, news items) for type only

Any coding agent that changes CLI behavior must run the live check and report
its summary before calling the work done. IDX ownership cases need a network
that Cloudflare does not block; when they return `403`, report that explicitly
instead of treating it as a pass.

## Migration Plan

Work happens on the `refactor/idx-core` branch. Each phase is its own commit
series with `nix flake check -L` green; `main` receives the branch only after
every phase passes both parity gates.

0. **Golden baseline.** Add `tests/golden.rs` and the golden files from the
   current `main`, plus `live-smoke.sh --compare`. No source changes.
1. **Agent-native output.** `auto` output, compact JSON, `jsonl`, stdin `-`,
   and exit-code classes. Touches only `config.rs`, `output/`, and `main.rs`.
2. **Workspace split, no behavior change.** Move code into `crates/idx-core`
   and `crates/idx-cli`.
3. **Pull logic into core.** Move the table above into core, add `CachePolicy`,
   `Fetched<T>`, `CoreConfig`, and typed filter enums.
4. **idx-mcp.** Implement the tool table over stdio, with fixture-backed tests.
5. **Release.** Workspace publishing, npm `idx-mcp`, release assets, flake package.

## Open Questions

1. Are the exit-code classes right, and should `compare` with partial results
   still exit `0`?
2. MCP tool granularity: is the grouped table above right, or should it be one
   tool per CLI command?
3. Should `ownership_sync` be available to agents by default?
4. Which source should back IHSG index, breadth, and sector data, and should it
   land before or after idx-mcp?
5. Is idx-api self-hosted only?
