# Architecture

## Domain Map

```
┌─────────────────────────────────────────────────────────┐
│                      CLI Layer                           │
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


### Capability Gating
```
MarketDataProvider = QuoteProvider + FundamentalsProvider
HistoryProvider    = separate trait, not all providers implement

Factory functions:
  default_provider(kind) → Box<dyn MarketDataProvider>
  history_provider(kind, mode, verbose) → Result<(ProviderKind, Box<dyn HistoryProvider>)>
```

## Ownership Module (SQLite-backed)

Unlike the `stocks` module (live-fetch), ownership is **import-then-query**:

1. `idx ownership import` — ETL pipeline: fetch PDF/API → parse → normalize → load SQLite
2. `idx ownership sync` — install a maintained SQLite snapshot via manifest + checksum validation
3. All query commands read from local `~/.local/share/idx/ownership.db`
4. Fully offline after import/sync

### Data Sources
- **KSEI** — official ≥1% shareholder registry, published monthly by IDX. Through May 2026 it was a PDF announcement; from the 2026-05-29 report onward it is an XLSX workbook on the IDX "Data Kepemilikan Saham" page
- **KSEI archive** — monthly ZIP/TXT balance-position matrix, used as a local fallback/backstop import path
- **Bing Finance** — global institutional ownership (REST API, quarterly)

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
Import: PDF/API → parse → normalize → resolve entities → SQLite INSERT
Query:  CLI args → SQLite SELECT → render table/json (no network)
```

## Configuration Precedence
```
CLI flags > environment variables > config file > defaults
```

## Error Strategy
- `IdxError` enum (thiserror) with structured error codes
- Table mode: human-readable error on stderr
- JSON mode: `{"error": true, "code": "...", "message": "..."}`
- Exit code 0 on success, non-zero on failure

## CLI Output and Offline Cache
- `version`, `cache info`, `cache clear`, and `ownership resolve map|merge` emit
  JSON objects when JSON output is selected.
- `stocks quote` and `stocks compare` require at least one symbol.
- Offline reads warn on stderr when serving cache entries past their TTL,
  including the cache fetch timestamp. `--quiet` suppresses these warnings.
  Staleness is not yet represented by a versioned JSON result envelope.
