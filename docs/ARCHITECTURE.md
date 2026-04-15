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
- **KSEI** — official ≥1% shareholder registry (monthly PDF from IDX)
- **KSEI archive** — monthly ZIP/TXT balance-position matrix, used as a local fallback/backstop import path
- **Bing Finance** — global institutional ownership (REST API, quarterly)

### Parser Pipeline
```
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
