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
- **MSN Finance** — default provider. Rich data: quotes, fundamentals, profile, earnings, financials, sentiment, insights, news, screener. No history for IDX stocks.
- **Yahoo Finance** — history fallback. Reliable OHLCV data via `/v8/finance/chart/`.

### Hybrid History Strategy
When `history_provider = auto` (default):
1. Check if current provider supports `HistoryProvider` trait
2. MSN doesn't → transparently fallback to Yahoo
3. Log info message: `"history provider fallback active (msn -> yahoo)"`

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
- **Bing Finance** — global institutional ownership (REST API, quarterly)

### Parser Pipeline
```
KSEI PDF → mutool stext (XML with coordinates) → quick-xml parse → KseiRawRow
  → normalize (ID locale numbers, dates, entity names) → KseiHolding
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
