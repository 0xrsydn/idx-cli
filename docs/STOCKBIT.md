# Stockbit Session Features: Research and Design

What the user's Stockbit session (`idx stockbit login`, see the Stockbit
sections of `docs/ARCHITECTURE.md`) can serve beyond the order book, live
quotes and daily history, and how idx should add it. Endpoints were found in
Stockbit's web bundle (`stockbit.com/_next/static/chunks/*.js`, the API client
module) and probed with a real session on Tue 2026-11-03 during trading hours,
one GET at a time at least 1 s apart. Nothing here ships yet unless marked.

## Rules for every session feature

1. **Read-only by construction.** The session client (`api/stockbit/exodus.rs`)
   issues GET on an allowlist of path prefixes, plus `POST /login/refresh`.
   The bundle also holds trading (orders, day trade, PIN, cash withdrawal,
   portfolio), chat, posting and account endpoints; idx never calls them, and
   the allowlist makes a stray call fail before it leaves the process.
2. **One client.** Every endpoint goes through `Exodus::get`: rotating refresh
   under the file lock, one retry after a 401 / JSON 403, the shared 1 s
   pacing, `AUTHERROR` / `UPSTREAMBLOCKED` / `RATELIMITED` mapping, and no
   token in logs, errors or cache keys.
3. **Stockbit-only commands use Stockbit whatever `--provider` says**, like
   `stocks intraday`; without a session they answer `AUTHERROR` (exit code as
   today). All endpoints below answer `401` without a session.
4. **Pure parsers, typed output.** Stockbit sends numbers as display strings
   (`"193,206"`, `"+0.0001"`, `"1.03B"`), dates as `"25 Mar 26"` or
   `YYYY-MM-DD`, and placeholders (`""`, `0`, `"0001-01-01 00:00:00"`). Parsers
   turn them into integers, decimals, `NaiveDate` and `None`, and are tested
   against recorded fixtures (`tests/fixtures/stockbit_exodus_*`).
5. **Units checked live** per endpoint before release (chart volume is lots,
   the order book is shares).
6. **Cache per endpoint**: book and quote 5 s, market state 30 s, movements
   1 h, corporate actions 6 h.
7. **Terms**: Stockbit's data is for personal, non-commercial use; the user
   docs (`web/src/content/docs/cli/stockbit.md`) say so for every feature.

## Endpoint inventory

All on `https://exodus.stockbit.com`, bearer access token, HTTP 200 on
2026-11-03 unless noted.

| Endpoint | Serves | Plan |
| --- | --- | --- |
| `/company-price-feed/v2/orderbook/companies/{SYM}` | quote, OHLC, foreign flow, depth | shipped (#74) |
| `/company-price-feed/market-time/session` | market state | shipped (#74) |
| `/charts/{SYM}/daily?timeframe=1m\|3m\|1y` | daily candles | shipped (#74) |
| `/charts/{SYM}/daily?timeframe=1w` | hourly candles, 5 sessions | P2 |
| `/corpaction/{type}[?symbol=SYM]` | corporate actions and calendar | **P1** |
| `/insider/company/majorholder` | insider / major-holder movements | **P1** |
| `/insider/shareholding/composition/companies/{SYM}` | holder composition by period | P2 |
| `/marketdetectors/{SYM}` | broker summary, Stockbit's bandar detector | P2 |
| `/analyst-ratings/{SYM}/consensus`, `/analyst-ratings/{SYM}` | estimates, ratings | P3 |
| `/order-trade/running-trade`, `/order-trade/trade-book` | trade tape | P3 |
| `/emitten-metadata/shareholders/{SYM}/chart` | composition chart | not probed: needs a second token from `POST /emitten-metadata/shareholders/token` |
| `/keystats/ratio/v1/{SYM}`, `/findata-view/company/financial` | ratios, statements | skip for now (MSN covers them) |

### Market state (`market-time/session`)

`data.detail` has two boards, `regular` and `fca` (full call auction), each
with `session`, `state_name`, `state_start_time`, `state_end_time`,
`time_left`, `is_last_session`, `is_end_of_day`. Observed:

| Time (WIB) | `regular` | `fca` |
| --- | --- | --- |
| Sat 2026-10-18 20:15 | `STATE_NAME_MARKET_CLOSED`, session 0, end of day | same |
| Tue 2026-11-03 12:35 | `STATE_NAME_BREAK`, session 1, 12:00:00-13:29:59 | `STATE_NAME_BREAK`, session 3, 12:00:00-13:59:59 |
| Tue 2026-11-03 13:30 | `STATE_NAME_SESSION_2`, session 2, 13:30:00-15:49:59 | still `STATE_NAME_BREAK` |
| Tue 2026-11-03 14:00 | `STATE_NAME_SESSION_2` | `STATE_NAME_SESSION_4` |

The order book and session quotes treat any numbered `STATE_NAME_SESSION_<n>`
as trading (#74; the assumed `STATE_NAME_MARKET_OPEN` never appeared). idx
reads `regular` only, so FCA-board stocks can show the regular board's state
around the break.

In session the order book's `fbuy`/`fsell`/`fnet` were the previous
session's (equal to IPOT's 2026-11-02 net for BBCA, BBRI, TLKM) with
`has_foreign_bs: false`; idx keeps them only when the flag is true (#74).
Other order-book fields not used yet: `total_bid_offer` (queued lots and
orders on each side), `market_data` (volume and value by board: regular,
negotiated, cash), `iepiev` (indicative equilibrium price and volume, for the
auctions), `corp_action.active`.

### Charts

- `timeframe=1d`, `PRICE_CHART_TYPE_CANDLE`: empty `prices` during the break.
- `timeframe=1d`, `PRICE_CHART_TYPE_LINE`: a rolling ~350 minutes of
  per-minute closes across two sessions, no OHLC or volume
  (`xaxisopt: "intraday"`). Adds nothing over the public `stocks intraday`.
- `timeframe=1w`, candle: hourly bars for 5 sessions, `interval_in_minutes:
  60`, volume in lots. The newest bar is stamped with its last update
  (`11:59:00`), not the hour. BBCA's three bars on 2026-11-03 summed to
  101.2M shares against 105.1M in the order book (pre-open auction or the
  negotiated market, unconfirmed).

Hourly history needs a datetime bar (`Ohlc.date` is a `NaiveDate`) and an
hourly `Interval`, across Yahoo, MSN, output, goldens and the MCP schema; that
is its own change.

## P1: corporate actions

idx has none today (only the `high-dividend` screen and Yahoo's dividend
yield). They explain price gaps in history (splits, bonuses, rights) and give
a calendar of cum dates.

### Endpoint

`GET /corpaction/{type}?symbol=SYM` gives one stock's history (BBCA: 37 cash dividends, cum dates 2009-06-09 to 2026-08-28); `GET /corpaction/{type}` without `symbol` is the
market-wide list, newest cum/event date first, upcoming events included. The
bundle passes `limit` (default 30); `rightissue` honoured it loosely (28 rows
for `limit=5`), `warrant` ignored it (1329 rows for `limit=3`), so idx filters
by date itself.

| `type` | Response key | Field prefix | Fields that matter |
| --- | --- | --- | --- |
| `dividend` | `dividend` | `dividend_` | `cumdate`, `exdate`, `recdate`, `paydate`, `value` (per share), `currency` |
| `stock_dividend` | `stock_dividend` | — | empty on 2026-11-03; record a row before parsing |
| `bonus` | `bonus` | `sahambonus_`, **`stocksplit_`** | `ratio` (`"6 : 4"`), `stocksplit_{cumdate,exdate,recdate,paymentdate,old,new,factor}` |
| `rightissue` | `rightissue` | `rightissue_` | `cumdate`, `exdate`, `recdate`, `old`, `new`, `ratio`, `price`, `factor`, `trading_start`, `trading_end` |
| `warrant` | `warrant` | `wrant_` | `trading_from`, `trading_end`, `exc_from`, `exc_end`, `exc_price` |
| `stocksplit` | `stocksplit` | `stocksplit_` | `cumdate`, `exdate`, `recdate`, `old`, `new`, `ratio`, `factor` |
| `reversesplit` | **`stock_reverse`** | — | empty on 2026-11-03; record a row before parsing |
| `rups` | `rups` | `rups_` | `date`, `time`, `venue`, `eligible_date` |
| `pubex` | `pubex` | `puexp_` | `date`, `time`, `venue` |
| `ipo` | `ipo` | `ipo_` | `listing_date`, `company_name`, `ipo_price.{minimum,maximum,final}`, `ipo_data_detail` (placeholders common) |
| `tenderoffer` | **`tender`** | `tender_` | `start`, `end`, `paydate`, `price`, `shares`, `percentage` |
| `economic` | — | — | macro calendar, not a corporate action; out of scope |

Examples from 2026-11-03: PPGL bonus 6:4 ex 2026-11-03; PRDA split 1:5 cum
2026-12-17; MGLV rights 100:15 at 8,880 cum 2026-11-04; BMAS rights 655:104 at
350 cum 2026-11-23, rights trading 2026-11-27 to 12-03; AUTO tender offer at
3,600 until 2026-11-11.

### Types

```rust
pub struct CorporateAction {
    pub symbol: String,          // BBCA.JK
    pub kind: ActionKind,        // Dividend, StockDividend, Bonus, Rights, Warrant,
                                 // Split, ReverseSplit, Meeting, PublicExpose, Ipo, TenderOffer
    pub cum_date: Option<NaiveDate>,
    pub ex_date: Option<NaiveDate>,
    pub record_date: Option<NaiveDate>,
    pub pay_date: Option<NaiveDate>,
    pub event_date: Option<NaiveDate>, // meeting, expose, listing, tender end
    pub detail: ActionDetail,    // kind-specific: amount + currency; old:new ratio
                                 // and factor; price; trading/exercise windows; venue
}
```

One parser per type maps its prefix into this record; unknown types and empty
lists are not errors. A row whose dates are all placeholders is dropped.

### Surface

- `idx stocks actions SYM [--kind dividend,rights,...] [--since DATE]`: one
  stock's history, newest first.
- `idx calendar [--kind ...] [--from DATE] [--to DATE]`: market-wide, default
  today to +30 days, sorted by the next relevant date.
- MCP: `get_corporate_actions {symbol, kinds?, since?}`,
  `get_corporate_calendar {kinds?, from?, to?}`.
- Later: `stocks history` notes ex-dates of splits, bonuses and rights in the
  window, so a gap is not read as a crash.

## P1: shareholder movements

### Endpoint

`GET /insider/company/majorholder?symbol=&insider=&date_start=&date_end=&page=`;
all parameters optional. 50 rows a page, `data.is_more` for the next page.
Without `symbol` it is market-wide, newest first: on 2026-11-03 the newest rows
were from 2026-11-02. BBCA's 50 rows span 2024-03-22 to 2026-03-25.

Each row (`data.movement[]`): `name`, `symbol`, `date` (`"25 Mar 26"`),
`previous` / `current` / `changes` `{value: shares as "193,206", percentage}`,
`action_type` (`ACTION_TYPE_BUY|SELL|TRANSFER`), `nationality`
(`NATIONALITY_TYPE_LOCAL|FOREIGN`), `badges` (`SHAREHOLDER_BADGE_DIREKTUR`,
`..._KOMISARIS`), `price.raw.value`, `value.raw.value` (IDR),
`is_estimated_price`, `purpose` (free text such as "Pembelian", "Investasi"),
`data_source.type` (`SOURCE_TYPE_IDX` for insider disclosures,
`SOURCE_TYPE_KSEI` for 5% holder changes), `broker_detail` (empty so far).

### Relation to `ownership`

The KSEI register idx imports is a monthly snapshot of holders above 1%. This
feed is the individual changes, days after the trade, including insiders far
below 1%. It joins the ownership entities by name (the same resolution
`ownership resolve` maintains), but needs no SQLite import: it is live.

### Signal quality

Discretionary insider buying, above all by several insiders together, is the
form of this data with predictive power in the literature; selling much less.
The raw feed is noisy:

- Program-like rows: BBCA's 2026-03-25 rows are every director and
  commissioner "buying" at the same 6,982, which reads as a share plan, not
  open-market conviction. Flag many holders on one date at one price.
- `TRANSFER` rows and estimated prices (`is_estimated_price`).
- The reporting lag: part of the move can be priced before the row appears.

So the command shows the rows faithfully, and a separate summary scores them:
net value by symbol, the number of distinct buyers (cluster), value against
average daily value, holders crossing 5% / 10%, and the program flag. Calling
it alpha waits for an event study (feed history back to 2024 per symbol,
Yahoo prices, forward 5/20/60-session returns against IHSG).

### Surface

- `idx ownership movements [SYM] [--from DATE] [--to DATE] [--holder NAME]
  [--action buy|sell|transfer] [--pages N]`: rows, newest first.
- `idx ownership movements --summary [--from DATE]`: the per-symbol score.
- MCP: `get_shareholder_movements {symbol?, from?, to?, holder?, pages?}`.

## P2 and P3

- **Composition by period** (`/insider/shareholding/composition/companies/{SYM}`,
  optional `period_start`, `period_end`, `view`): per `report_date` (latest
  2026-10-30 on 2026-11-03), total shares and each holder's shares,
  percentage and previous shares. Fresher than waiting on the IDX register;
  could feed `ownership changes` as a source.
- **Market detector** (`/marketdetectors/{SYM}`): `bandar_detector` with
  `accdist` labels per top-N, broker counts, average price. A second source
  for `stocks broker` / `bandar` beside IPOT (`--source ipot|stockbit`), as
  `flow` has `--source ipot|idx`; IPOT answered 502 on 2026-10-31.
- **Hourly candles** (`timeframe=1w`), see "Charts".
- **Analyst consensus**: revenue and operating profit actuals and estimates
  by year; ratings per broker.
- **Trade tape**: running trade and trade book; large and only meaningful in
  session; wait for `--watch`.

## Order of work

1. Close #74: the open-state name in trading hours, fixtures for the observed
   states (done 2026-11-03).
2. Corporate actions (parsers, `stocks actions`, `calendar`, MCP, goldens).
3. Shareholder movements, rows first, then `--summary`.
4. Event study for the movement score; P2 by demand.

## Open questions

- The `regular` state in session 1 (assumed `STATE_NAME_SESSION_1`), the
  pre-open (08:45-09:00) and the closing auction (15:50-16:00).
- When `has_foreign_bs` turns true, and whether the figures are then the
  day's.
- `stock_dividend` and `reversesplit` row shapes (both empty on 2026-11-03).
- Whether `/corpaction/{type}` takes a date range (the bundle sends only
  `limit` and `symbol`).
- The reporting lag between a movement's `date` and its appearance in the
  feed (sample the market-wide first page daily).
