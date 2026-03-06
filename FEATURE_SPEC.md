# Feature Spec: MSN Finance Full Coverage

**Branch:** `feat/msn-full`
**Status:** Draft — pending review
**Reference:** `origin/dev/rubick` (Go implementation by rubick)

---

## Background

The Rust CLI currently supports two MSN endpoints:
- `Finance/Quotes` → `quote()`
- `api.msn.com/keyratios` → `fundamentals()`

The rubick Go project (friend's scraper) demonstrates a much wider set of MSN Finance endpoints covering equities, financials, earnings, charts, sentiment, insights, and news — all using the same public API key. This spec defines the full porting roadmap from Go → Rust.

MSN API key (public, embedded in MSN Money website):
```
0QfOX3Vn51YCzitbLaRkTTBadtWpgTN8NZLW0C1SEM
```

Base URLs:
- `https://assets.msn.com/service/` — core market data (Quotes, Charts, Equities, Earnings, Sentiment, Screener)
- `https://api.msn.com/msn/v0/pages/finance/` — extended data (keyratios, insights, newsfeed)
- `https://services.bingapis.com/contentservices-finance.hedgefunddataprovider/api/v1/` — Bing ownership data

---

## Endpoints to Implement

### P0 — Core Completeness

#### 1. `Finance/Equities` — Company Profile
- **Method:** GET
- **URL:** `{MSN_ASSETS_BASE_URL}Finance/Equities?apikey={key}&ids={id}&wrapodata=false`
- **Returns:** `EquityData` — company name, description, sector, industry, website, employees, address, officers/executives
- **CLI use:** `idx stock profile BBCA` or folded into `info` subcommand
- **Rust struct:**
```rust
pub struct EquityData {
    pub id: String,
    pub symbol: String,
    pub short_name: String,
    pub long_name: String,
    pub description: String,
    pub sector: String,
    pub industry: String,
    pub website: String,
    pub employees: i64,
    pub address: String,
    pub city: String,
    pub country: String,
    pub phone: String,
    pub officers: Vec<Officer>,
}

pub struct Officer {
    pub name: String,
    pub title: String,
    pub age: Option<i32>,
    pub year_born: Option<i32>,
    pub total_pay: Option<i64>,
}
```
- **Complexity:** Low

---

#### 2. `Finance/Equities/financialstatements` — Financial Statements
- **Method:** GET
- **URL:** `{MSN_ASSETS_BASE_URL}Finance/Equities/financialstatements?apikey={key}&ids={id}&wrapodata=false`
- **Returns:** Balance sheet, cash flow, income statement — each as a map of `{field: value}` keyed by line item name, with period metadata (reportDate, endDate, currency, source)
- **CLI use:** `idx stock financials BBCA [--statement income|balance|cashflow]`
- **Note:** Fields are dynamic (map-based), not fixed columns — render as table with row=line item, col=period if multiple periods returned
- **Rust struct:**
```rust
pub struct FinancialStatements {
    pub instrument: InstrumentInfo,
    pub balance_sheet: Option<BalanceSheet>,
    pub cash_flow: Option<CashFlow>,
    pub income_statement: Option<IncomeStatement>,
}

pub struct BalanceSheet {
    pub current_assets: HashMap<String, f64>,
    pub long_term_assets: HashMap<String, f64>,
    pub current_liabilities: HashMap<String, f64>,
    pub equity: HashMap<String, f64>,
    pub currency: String,
    pub report_date: String,
    pub end_date: String,
}

// Similar pattern for CashFlow (financing/investing/operating) and IncomeStatement
```
- **Complexity:** Medium (dynamic maps → table rendering)

---

### P1 — High Analyst Value

#### 3. `Finance/Events/Earnings` — Earnings History & Forecast
- **Method:** GET
- **URL:** `{MSN_ASSETS_BASE_URL}Finance/Events/Earnings?apikey={key}&ids={id}&wrapodata=false`
- **Returns:**
  - `EpsLastYear`, `RevenueLastYear`
  - `Forecast.annual` — 2 forward years: EpsForecast, RevenueForecast, GAAP/Normalized consensus
  - `Forecast.quarterly` — next 4 quarters with same fields + EarningReleaseDate
  - `History.annual` — 5 years: EpsActual, EpsSurprise, EpsSurprisePercent, RevenueActual, RevenueSurprise
  - `History.quarterly` — ~12 quarters of actuals + surprises
- **CLI use:** `idx stock earnings BBCA [--forecast|--history] [--annual|--quarterly]`
- **Rust struct:**
```rust
pub struct EarningsReport {
    pub eps_last_year: f64,
    pub revenue_last_year: f64,
    pub forecast: EarningsForecast,
    pub history: EarningsHistory,
}

pub struct EarningsData {
    pub eps_actual: Option<f64>,
    pub eps_forecast: Option<f64>,
    pub eps_surprise: Option<f64>,
    pub eps_surprise_pct: Option<f64>,
    pub revenue_actual: Option<f64>,
    pub revenue_forecast: Option<f64>,
    pub revenue_surprise: Option<f64>,
    pub earning_release_date: Option<String>,
    pub period_type: String, // e.g. "Q42025", "2025"
}
```
- **Complexity:** Medium (nested map keyed by period string)

---

#### 4. `Finance/Charts` — Price Chart / OHLCV History
- **Method:** GET
- **URL:** `{MSN_ASSETS_BASE_URL}Finance/Charts?apikey={key}&ids={id}&chartType={type}&wrapodata=false`
- **Chart types:** `1D`, `1W`, `1M`, `3M`, `6M`, `1Y`, `3Y`, `5Y`, `MAX`
- **Returns:** Series of `ChartPoint { time, open, high, low, close, price, volume }`
- **Note:** This unblocks the `history()` provider method — current implementation explicitly returns `Unsupported`. MSN charts don't guarantee OHLCV on all timeframes (1D is often price-only), so parse defensively.
- **CLI use:** `idx stock history BBCA --period 3M` (existing command, just needs this wired up)
- **Rust struct:**
```rust
pub struct ChartPoint {
    pub time: String,
    pub open: Option<f64>,
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub close: Option<f64>,
    pub price: f64,
    pub volume: Option<i64>,
}
```
- **Complexity:** Medium (parse series array, handle missing OHLCV gracefully)

---

### P2 — Enrichment Layer

#### 5. `Finance/SentimentBrowser` — Crowd Sentiment
- **Method:** GET
- **URL:** `{MSN_ASSETS_BASE_URL}Finance/SentimentBrowser?apikey={key}&ids={id}&wrapodata=false`
- **Returns:** Per-period sentiment stats: bullish/bearish/neutral counts, time range name (e.g., "1D", "1W", "1M")
- **CLI use:** `idx stock sentiment BBCA`
- **Rust struct:**
```rust
pub struct SentimentData {
    pub symbol: String,
    pub statistics: Vec<SentimentPeriod>,
}

pub struct SentimentPeriod {
    pub time_range: String,   // "1D", "1W", "1M"
    pub bullish: i32,
    pub bearish: i32,
    pub neutral: i32,
}
```
- **Complexity:** Low

---

#### 6. `api.msn.com/insights` — AI-Generated Insights
- **Method:** GET
- **URL:** `{MSN_API_BASE_URL}insights?apikey={key}&ids={id}&wrapodata=false`
- **Returns:** Summary text, highlights array, risks array, last updated timestamp
- **CLI use:** `idx stock insights BBCA`
- **Rust struct:**
```rust
pub struct InsightData {
    pub id: String,
    pub summary: String,
    pub highlights: Vec<String>,
    pub risks: Vec<String>,
    pub last_updated: String,
}
```
- **Complexity:** Low

---

#### 7. `MSN/Feed/me` — Stock News Feed
- **Method:** GET
- **URL:** `{MSN_API_BASE_URL}` + entity feed params with stock ID
- **Returns:** News cards: title, URL, abstract, provider name, publish time, read time
- **CLI use:** `idx stock news BBCA [--limit 10]`
- **Rust struct:**
```rust
pub struct NewsItem {
    pub id: String,
    pub title: String,
    pub url: String,
    pub description: String,
    pub provider: String,
    pub published_at: String,
    pub read_time_min: Option<i32>,
}
```
- **Complexity:** Medium (URL construction + response parsing needs rubick reference)

---

#### 8. `Finance/Screener` — IDX Universe Screener
- **Method:** POST
- **URL:** `{MSN_ASSETS_BASE_URL}Finance/Screener?apikey={key}&wrapodata=false`
- **Body:** `{ filter: [{key, keyGroup, isRange}], order: {key, dir}, returnValueType: [...], screenerType: "...", limit: 50 }`
- **Returns:** List of stocks with quote data (price, change, market cap, volume, 52w hi/lo, YTD return)
- **CLI use:** `idx screen [--preset top-gainers|top-losers|most-active|...]`
- **Complexity:** Medium (POST body construction, preset filter definitions)

---

### P3 — Optional / Future

#### 9. Bing Ownership API — Institutional Holders
- **Base:** `https://services.bingapis.com/contentservices-finance.hedgefunddataprovider/api/v1/`
- **Endpoints:**
  - `GetSecurityTopShareHolders`
  - `GetSecurityTopBuyers` / `GetSecurityTopSellers`
  - `GetSecurityTopNewShareHolders` / `GetSecurityTopExitedShareHolders`
- **CLI use:** `idx stock holders BBCA [--buyers|--sellers|--new|--exited]`
- **Note:** Separate base URL, may need different auth/headers than MSN. Validate working before implementing.
- **Complexity:** Low-Medium

---

## Implementation Plan

### Phase 1 — Extend `src/api/msn/`
1. Add `fetch_equities(symbol)` to `client.rs`
2. Add `fetch_financial_statements(symbol)` to `client.rs`
3. Add `fetch_earnings(symbol)` to `client.rs`
4. Add `fetch_charts(symbol, period)` to `client.rs`
5. Add corresponding parse functions to `parse.rs`
6. Expose via new methods on `MsnProvider` in `mod.rs`

### Phase 2 — New Rust structs in `src/api/msn/types.rs` (new file)
- Extract shared types (currently inline in `parse.rs`) into dedicated `types.rs`
- Add all new structs listed above

### Phase 3 — Wire CLI commands in `src/cli/stocks.rs`
New subcommands to add:
```
idx stock profile <SYMBOL>        # Company info + officers
idx stock financials <SYMBOL>     # Income / balance / cashflow
idx stock earnings <SYMBOL>       # EPS history + forecast
idx stock sentiment <SYMBOL>      # Crowd sentiment
idx stock insights <SYMBOL>       # AI highlights + risks
idx stock news <SYMBOL>           # News feed
idx screen                        # IDX screener (separate top-level command)
```

And unblock existing:
```
idx stock history <SYMBOL>        # Wire MSN charts (currently Unsupported)
```

### Phase 4 — Output formatting
- Table output for financials (line item rows, period columns)
- Compact output for earnings (actual vs forecast vs surprise %)
- JSON output flag `--json` should work for all new commands

---

## Open Questions

1. **Chart OHLCV completeness** — rubick notes that MSN charts don't always return full OHLCV on short timeframes (e.g., 1D is price-only). Do we want to keep `history()` returning `Unsupported` for MSN and add a separate `charts()` method, or silently map price → close for compatibility?

2. **Financial statements period count** — The API returns one period per call (most recent). Do we want to add a bulk-fetch loop (e.g., fetch last 4 quarters separately) or just expose single-period for now?

3. **News feed URL construction** — needs exact param structure from rubick's `GetNewsFeed()` Go implementation. Worth a closer look before implementing.

4. **Screener presets** — rubick defines filter key constants (e.g., `"st_list_topperfs"`, `"st_reg_id"`). Need to decide which presets to expose as CLI flags and what the default screener view looks like.

5. **Provider trait extension** — `quote()`, `fundamentals()`, `history()` are currently defined on `Provider` trait. New methods (earnings, profile, etc.) are MSN-specific — do we extend the trait or expose them as inherent methods on `MsnProvider` only?

---

## Files to Touch

```
src/api/msn/
  client.rs      — add fetch_* methods
  mod.rs         — expose new provider methods
  parse.rs       — add parse_* functions
  types.rs       — NEW: shared type definitions

src/cli/
  stocks.rs      — add new subcommands + output formatting

tests/
  cli.rs         — integration tests for new commands
  fixtures/      — add response fixtures for new endpoints
```

---

*Drafted by Ciphercat based on rubick Go implementation analysis + live MSN API verification.*
