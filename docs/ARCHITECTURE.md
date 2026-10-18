# Architecture

## Crates

The repo is a Cargo workspace with one shared version:

- `crates/idx-core` (library): the `Idx` entry point with `market()`,
  `financial()`, `analysis()`, and `investor()` (cache policy and `Fetched<T>` in
  `client.rs`; ownership queries, import, sync, and discovery in `investor.rs`),
  providers (`api/`), `analysis/`, `cache`, `config` (`CoreConfig`),
  `ownership/` (SQLite, parsers, discovery, snapshot sync), `error`, and
  `runtime` (quiet/info/warn). It owns the shared test fixtures in
  `tests/fixtures/`; the fixture-backed mock provider needs the `testing` feature.
- `crates/idx-cli` (binary `idx`): clap commands (`cli/`), output format and
  color resolution (`config.rs`, `IdxConfig`), and rendering (`output/`). Its
  `tests/` hold the CLI integration and golden output suites.

`crates/idx-cli/src/cli/` holds no fetching, caching, or database logic: each
command parses arguments, makes one idx-core call, and renders the result.

## Domain Map

```
┌─────────────────────────────────────────────────────────┐
│                 CLI Layer (idx-cli)                      │
│  main.rs → cli/stocks.rs, cli/ownership.rs, cli/...     │
│  Clap derive structs, command dispatch, arg validation   │
└──────────────────────┬──────────────────────────────────┘
                       │
        ┌──────────────┼──────────────┐
        │              │              │
┌───────▼───────┐ ┌────▼────┐ ┌──────▼──────┐
│  API Layer    │ │ Analysis│ │  Ownership  │
│  (providers)  │ │  Module │ │   Module    │
│               │ │         │ │             │
│ MarketData    │ │ techni- │ │ SQLite DB   │
│ Provider trait│ │ cal.rs  │ │ KSEI parser │
│               │ │ signals │ │ Bing client │
│ Yahoo (OHLCV) │ │ fund.rs │ │ entity res. │
│ MSN (rich)   │ │         │ │ FTS5 search │
└───────┬───────┘ └────┬────┘ └──────┬──────┘
        │              │              │
┌───────▼──────────────▼──────────────▼──────┐
│              Output Layer                    │
│  table.rs (comfy-table) │ json.rs (serde)   │
└─────────────────────────────────────────────┘
```

## Provider Architecture

### Dual Provider Model
- **MSN Finance** — default provider. Rich data: quotes, fundamentals, profile, earnings, financials, sentiment, insights, news, screener, and explicit price-only chart history for supported IDX windows.
- **Yahoo Finance** — default/auto history source. Reliable OHLCV data via `/v8/finance/chart/`.
- **IPOT (Indo Premier)** — broker summaries, whichever provider is selected
  (`BrokerSummaryProvider`). See [Broker Summaries](#broker-summaries).
- **IDX (idx.co.id)** — the daily trading summary behind foreign flow
  (`TradingSummaryProvider`). See [Foreign Flow](#foreign-flow).

### MSN Endpoints
- `https://assets.msn.com/service/`: quotes, charts, equities, earnings,
  sentiment, screener
- `https://api.msn.com/msn/v0/pages/finance/`: key ratios, insights, news feed

The public API key embedded in MSN Money lives in `crates/idx-core/src/api/msn/client.rs`.

### Hybrid History Strategy
When `history_provider = auto` (default):
1. Use Yahoo for history because it provides full OHLCV candles.
2. Keep logging when provider selection falls back from MSN to Yahoo.
3. Allow explicit `--history-provider msn` for supported MSN chart windows (`1mo`, `3mo`, `1y` with `1d` interval).

MSN Charts are price-only for IDX. The CLI normalizes them into `Ohlc` rows by
using the chart price as open/high/low/close and `0` volume.
MSN candle dates use WIB (UTC+7); Yahoo candle dates use `meta.gmtoffset`, with
WIB as the fallback.

### Quote and Screener Data
- Quotes expose nullable `as_of` last-trade timestamps in WIB. Table output flags
  quotes whose last trade was more than seven days ago.
- `change`, `change_pct`, and `volume` are nullable when the provider omits them;
  JSON consumers must not assume these fields are always numeric.
- Listed MSN instruments without a price return `NOMARKETDATA`; unknown symbols
  still return `SYMBOLNOTFOUND`. Empty sentiment responses represent no votes.
- Performers, high-volume, and large-cap screens fetch the complete candidate
  list before local ranking and `--limit`. MSN ignores `pageIndex` for these
  lists, so the provider uses the response's `count` to request the full list
  when the initial batch is incomplete, and rejects a still-truncated response.
- Full-list screen caches use an `all` key, separate from older limited results.

### Broker Summaries
- `GET https://www.indopremier.com/module/saham/include/data-brokersummary.php`
  takes `code`, `start`/`end` (`MM/DD/YYYY`), `fd` (`all|F|D`) and `board`
  (`all|RG|TN|NG`). It needs no login and answers an HTML fragment: the top 10
  buyers and top 10 sellers ranked by lots (gross, so a broker can be on both
  sides), plus range totals. `api/ipot/parse.rs` parses it purely.
- Figures of a million and more arrive abbreviated (`1.6 M`); they are scaled
  in decimal and the summary's `rounded` flag says so. `F. NVal` ignores the
  investor and board filters, so `foreign_net_value` is null for a filtered
  summary. With `fd=F` or `fd=D` the footer covers only that side's buying
  (BBCA 2026-10-16 foreign: `T. Val` 1.6 T while seller CC alone sold
  2.0 T), so totals are null for an investor-filtered summary.
- An unknown code is a 200 with `Invalid stock code!` (`SYMBOLNOTFOUND`). The
  server also answers spurious nginx 404s for seconds at a time, so the client
  retries 404 and 5xx with backoff. A range without trades is an empty summary.
- Data is end of day: IDX hides broker codes during trading. Summaries that
  ended before today are cached for 7 days, today's for 5 minutes. Broker
  names come from `broker/codes.tsv` (IDX `GetBrokerCodeList`).
- Bandarmology (`analysis/bandar.rs`, method `top_concentration_v1`) is pure
  over one summary: for the top 1/3/5/10 per side, buy value minus sell value
  in basis points of value traded, with thresholds ±500/±1500 bps taken from
  fzn0x/idx. A tier with a missing figure has no reading; missing values are
  never summed as zero. A change to the rules gets a new method name.

### Foreign Flow
- `GET https://www.idx.co.id/primary/TradingSummary/GetStockSummary`
  `?length=9999&start=0&date=YYYYMMDD` answers one JSON row per stock for the
  session: prices, volume, value, `ForeignBuy`/`ForeignSell` (shares) and
  non-regular volume and value. `Volume`, `Value` and the foreign figures are
  the regular board only: IPOT's regular-board total for BBCA on 2026-10-16
  matches `Value`, and in a week of every stock no foreign figure exceeded
  regular volume. `non_regular_bps` is therefore over regular plus
  non-regular value. A day without trading has no rows; rows dated
  another day are dropped. `api/idx/parse.rs` parses it purely.
- Cloudflare rejects most TLS fingerprints (in October 2026 only
  `curl_safari184` passed), so the request uses curl-impersonate's profile
  fallback with a 30 s limit. A challenge page fails that profile, not the
  parse.
- Each session is cached whole (every stock) by date: 30 days once past,
  5 minutes for today. `Flow::foreign` walks weekdays without requesting
  weekends; offline, `--days` skips uncached sessions while a `--from/--to`
  range fails on one. `foreign_net_value` is net shares at the session's
  average price (value over volume), an estimate.

### Capability Gating
```
MarketDataProvider = QuoteProvider + FundamentalsProvider
HistoryProvider    = separate trait, not all providers implement

Factory functions:
  default_provider(kind) → Box<dyn MarketDataProvider>
  history_provider(kind, mode, verbose) → Result<(ProviderKind, Box<dyn HistoryProvider>)>
  broker_provider() → Box<dyn BrokerSummaryProvider>
  trading_summary_provider() → Box<dyn TradingSummaryProvider>
```

## Ownership Module (SQLite-backed)

Unlike the `stocks` module (live-fetch), ownership is **import-then-query**:

1. `idx ownership import` — ETL pipeline: fetch XLSX/PDF → parse → normalize → load SQLite
2. `idx ownership sync` — install a maintained SQLite snapshot via manifest + checksum validation
3. All query commands read from local `~/.local/share/idx/ownership.db`
4. Fully offline after import/sync

### Data Sources
- **KSEI** — official ≥1% shareholder registry, published monthly by IDX. Through May 2026 it was a PDF announcement; from the 2026-05-29 report onward it is an XLSX workbook on the IDX "Data Kepemilikan Saham" page
- **KSEI archive** — monthly ZIP/TXT balance-position matrix, used as a local fallback/backstop import path
- **Bing Finance** — global institutional ownership. The schema, `--source bing`
  queries, and `crates/idx-core/src/api/msn/bing.rs` exist, but `ownership import --fetch-bing`
  is intentionally unsupported, so no ingest path populates Bing rows today

### Discovery
`idx ownership discover` merges two IDX sources:
- the announcement API (`GetAllAnnouncement`) for the legacy PDF announcements
- the Data Kepemilikan Saham page (`/id/perusahaan-tercatat/data-kepemilikan-saham/`),
  whose server-rendered Nuxt payload lists every XLSX with a description such as
  `Pemegang Saham di Atas 1% per 31 Agustus 2026`; the as-of date comes from that text

Only the above-1% XLSX is `supported`; the daily above-5% and investor-type workbooks
are listed as `unsupported`. Both URLs can be overridden for tests with
`IDX_OWNERSHIP_ANNOUNCEMENT_API_URL` and `IDX_OWNERSHIP_DATA_PAGE_URL`.

IDX sits behind Cloudflare, which accepts some curl-impersonate TLS profiles and
rejects others per path (for example `curl_chrome142` gets 403 on the data page).
IDX fetches therefore try several profiles until the response validates (JSON, page
payload, `%PDF`, or `PK` zip magic). `IDX_CURL_IMPERSONATE_BIN` pins a single profile.

### Parser Pipeline
```
KSEI XLSX → zip + quick-xml (sharedStrings + sheet1) → exact 12-column header check
  → KseiHoldingDraft (Excel serial dates, numeric shares, percent → bps)
  → SQLite INSERT (within transaction)

KSEI PDF → mutool stext (XML with coordinates) → quick-xml parse → KseiRawRow
  → normalize (ID locale numbers, dates, entity names) → KseiHolding
  → SQLite INSERT (within transaction)

KSEI archive ZIP/TXT → pipe-delimited balance-position rows
  → map investor-type/locality buckets into synthetic aggregate holders
  → SQLite INSERT (within transaction)
```

## Data Flow Patterns

### Live Query (stocks module)
```
CLI args → resolve symbol → provider.quote/history/fundamentals → render table/json
```

### Import-Query (ownership module)
```
Import: XLSX/PDF/archive → parse → normalize → resolve entities → SQLite INSERT
Query:  CLI args → SQLite SELECT → render table/json (no network)
```

## Configuration Precedence
```
CLI flags > environment variables > config file > defaults
```

## Error Strategy
- `IdxError` enum (thiserror) with structured error codes
- Table mode: human-readable error on stderr
- JSON and JSONL modes: `{"error": true, "code": "...", "message": "..."}` on stderr, stdout empty.
  This includes argument errors (`INVALIDINPUT`, exit 2) when `-o`/`--output`,
  `IDX_OUTPUT`, or auto mode with stdout not a terminal selects JSON; `--help`
  and `--version` print normally.
- Untrusted text (provider strings, KSEI names) has control characters and
  bidirectional overrides replaced before it reaches a terminal table.
- Exit codes by class: `0` success, `2` `INVALIDINPUT`/`CONFIGERROR` and clap usage
  errors, `3` `SYMBOLNOTFOUND`/`NOMARKETDATA`/`CACHEMISS`, `4` `HTTP`/`RATELIMITED`/
  `PROVIDERUNAVAILABLE`/`OFFLINE`/`AUTHERROR`, `5` `UNSUPPORTED`, `1` everything else
- A totally failed `stocks compare` emits one structured error in JSON mode; per-symbol warnings are retained for partial success, not printed ahead of a total-failure error.

## CLI Output and Offline Cache
- Output format resolves as `-o` flag > `IDX_OUTPUT` > `general.output` > `auto`.
  `auto` is a table when stdout is a terminal and JSON otherwise
  (`std::io::IsTerminal`).
- JSON is pretty-printed only on a terminal; piped output is compact. `jsonl`
  prints each array item on its own line with the same key order as `json`,
  and a single object as one line.
- ANSI styling goes through `output::style` and is disabled when stdout is not a
  terminal or color is turned off.
- `version`, `cache info`, `cache clear`, and `ownership resolve map|merge` emit
  JSON objects when JSON output is selected.
- `stocks quote` and `stocks compare` require at least one symbol.
- Offline reads warn on stderr when serving cache entries past their TTL,
  including the cache fetch timestamp. `--quiet` suppresses these warnings.
  In JSON mode warnings and info are JSON lines,
  `{"level": "warning", "message": "..."}`, so stderr stays parseable.
  Staleness is not yet represented by a versioned JSON result envelope.
