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
- **IDX (idx.co.id)** — the daily trading summary behind `stocks flow
  --source idx` (`TradingSummaryProvider`). See [Foreign Flow](#foreign-flow).
- **Stockbit (public page)** — `stocks intraday` whichever provider is
  selected (`IntradayProvider`), and quotes under the opt-in
  `--provider stockbit`. See [Stockbit](#stockbit).

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
- Quotes expose nullable `as_of` last-trade timestamps in WIB, and
  `behind_session` when `as_of` is before the latest weekday whose session has
  ended (16:15 WIB). It is computed on return, not cached; MSN can lag the close
  by hours, and on an exchange holiday every quote reads as behind. Only `.JK`
  quotes and `--region id` screens are marked; other exchanges keep other hours.
  Table output notes behind quotes, and calls those over seven days old not
  current.
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
  sides), plus range totals. `api/ipot/parse.rs` parses it purely, with
  html5ever (`scraper`), so it reads the cells, date inputs and footer a
  browser would build: a hand-written reader kept disagreeing with HTML on
  crafted markup in the 0.4.0 review. A table whose rows don't line up
  (missing cells, ranks out of order, a broker after a blank row, a second
  body) rejects the page, and the dates it echoes must match the request.
- Figures of a million and more arrive abbreviated (`1.6 M`); they are scaled
  in decimal and the summary's `rounded` flag says so. `F. NVal` ignores the
  investor and board filters, so `foreign_net_value` is null for a summary of
  one board. With `fd=F` or `fd=D` the footer covers only that
  side's buying (BBCA 2026-10-16: foreign 1.7 T and domestic 4.9 T of 6.6 T),
  so `total_value` is null and the footer is `buy_value`. On all boards the
  side's `sell_value` is its buying less the foreign net (foreign: 1.7 + 2.3
  = 4.0 T, against IDX's 3.95 T regular) or plus it (domestic); with a board
  filter it is null.
- An unknown code is a 200 with `Invalid stock code!` (`SYMBOLNOTFOUND`). The
  server also answers spurious nginx 404s for seconds at a time, so the client
  retries 404 and 5xx with backoff. A range without trades is an empty summary.
- Data is end of day: IDX hides broker codes during trading. Summaries that
  ended before today are cached for 7 days, today's for 5 minutes. Broker
  names come from `broker/codes.tsv` (IDX `GetBrokerCodeList`).
- Bandarmology (`analysis/bandar.rs`, method `top_concentration_v1`) is pure
  over one summary: for the top 1/3/5/10 per side, the buyers' share of the
  value bought minus the sellers' share of the value sold (for all investors,
  buy value minus sell value in basis points of value traded), with thresholds ±500/±1500 bps taken from
  fzn0x/idx. A tier with a missing figure has no reading; missing values are
  never summed as zero. A change to the rules gets a new method name.

### Foreign Flow
- The default source is IPOT: per session, the all-investor broker summary
  gives the value traded and the foreign one (`fd=F`) gives foreign buying
  (its footer), the foreign net, and selling (buying less the net), all
  boards, through the broker summary cache. IPOT's foreign net matched IDX's
  estimate within 0.3 B on BBCA 2026-10-12 to 16. An empty all-investor
  summary counts as no session.
- `--source idx`: `GET https://www.idx.co.id/primary/TradingSummary/GetStockSummary`
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

### Stockbit
- `GET https://stockbit.com/symbol/{CODE}` needs no login. Next.js
  server-renders the quote and today's chart into the page's `__NEXT_DATA__`
  JSON; `api/stockbit/parse.rs` reads it purely. `props.pageProps.company`
  is the quote (numbers mostly as strings, an index price as a float string),
  `orderbook.{bid,offer}.price` the best bid and offer, `market_hour.status`
  the market's state, and `status` `STATUS_ACTIVE|SUSPENDED|DELISTED`.
  `chart.prices[]` holds one price per minute in WIB with no offset, after a
  synthetic first row at the previous close; per-minute OHLC and volume are
  empty. An unknown code is HTTP 404 with the 404 page.
- Stockbit's JSON API (`exodus.stockbit.com`) answers 401 without a user's
  bearer token; it is used only with the user's own session (below).
- Bid and offer are kept only while `market_hour.status` is `open`, so a
  closing book never reads as a live one. Observed 2026-10-23: `open` during
  trading (bid/offer set), `break` from 11:30 on a Friday, `close` after
  hours; any other value fails closed. In session the chart gained a point
  every minute and the last trade was within a minute of the clock, although
  Stockbit's terms call its data delayed at least 10 minutes. Order-book
  sizes are not shown; they are shares (BBCA's best bid on 2026-10-23 was
  1,357,500 on the page and 13,575 lots in the session API's book). `as_of`
  comes from `date`/`time`
  only when the stock traded (a suspended stock carries today's date).
  `session_note` repeats what the page says (suspended, delisted) or "no
  trades this session"; it is never inferred.
- CloudFront in front of the page answers a static 403 to some user agents
  (`curl/`, `axios/`, `Go-http-client/`, empty) and does not fingerprint TLS,
  so a plain ureq request with `idx-cli/<version> (+repo)` passes. A 403 is
  `UPSTREAMBLOCKED` and is not retried. Requests are at least 1 s apart
  across the user's idx processes (the last request's time sits in
  `<cache>/stockbit.pace` under a file lock, so a shell loop of `idx` calls is
  paced too), and snapshots are cached for 5 s (at most `quote_ttl`), which
  coalesces an agent's quote-then-intraday burst across MCP calls.
- Stockbit's terms call its data delayed at least 10 minutes and for
  personal use; frontends say so. Fixtures (`tests/fixtures/stockbit_*`) are
  pages recorded 2026-10-22 after the close, cut to the fields read.

### Stockbit session (opt-in login)
- `idx stockbit login` stores the user's session from a pasted
  `credentialStorage` cookie (or a bare refresh token) in
  `<config dir>/stockbit-session.json`, written atomically with mode 0600;
  `IDX_STOCKBIT_REFRESH_TOKEN` / `IDX_STOCKBIT_ACCESS_TOKEN` fill fields the
  file lacks. In a terminal it first serves a page on 127.0.0.1
  (`api/stockbit/loopback.rs`): a bookmarklet that reads the cookie in the
  user's own browser and opens the page with it in the URL fragment, or a
  clipboard button; a per-run secret guards the path, the page is `no-store`
  with a strict CSP, and the server stops on success or after five minutes.
  Piped stdin keeps the paste path. `api/stockbit/auth.rs` reads expiries
  from the JWTs (`exp`,
  unverified) and refreshes an access token within an hour of expiry with
  `POST /login/refresh` (bearer = refresh token), which rotates both tokens.
  Because a refresh token is spent once, refreshes run under an exclusive
  lock on `stockbit-session.lock` and re-read the file first, so concurrent
  idx processes never spend the same token. A data request answered 401 (or
  a JSON 403) refreshes once and retries once; a second refusal is
  `AUTHERROR`. Tokens never reach output, errors, logs or cache keys
  (`Session`'s `Debug` redacts them).
- `api/stockbit/exodus.rs` (endpoints and units mapped by fzn0x/idx, MIT;
  its recordings are `tests/fixtures/stockbit_exodus_*`):
  `/company-price-feed/v2/orderbook/companies/{CODE}` gives the depth ladder
  (`volume` in shares, `que_num` orders), OHLC, value, frequency and foreign
  buy/sell/net; `/company-price-feed/market-time/session` the market state
  (`STATE_NAME_MARKET_CLOSED`, cached 30 s per process); `/charts/{CODE}/daily`
  daily candles for `1m`/`3m`/`1y` windows (volume in lots, returned in
  shares). Requests share the public page's cross-process pacing.
- With a session, `--provider stockbit` quotes come from the order book (bid
  and offer only in `STATE_NAME_MARKET_OPEN`, a name assumed until observed
  in session), `stocks orderbook` works, and `auto` history under the stockbit
  provider uses Stockbit's daily windows (`5d` to `1y`, trimmed from `1m`,
  `3m` or `1y`), Yahoo otherwise. Without one, quotes use the public page and
  the order book is `AUTHERROR`.

### Capability Gating
```
MarketDataProvider = QuoteProvider + FundamentalsProvider
HistoryProvider    = separate trait, not all providers implement

Factory functions:
  default_provider(kind) → Box<dyn MarketDataProvider>
  history_provider(source, verbose) → Box<dyn HistoryProvider>   (source from history_source)
  broker_provider() → Box<dyn BrokerSummaryProvider>
  trading_summary_provider() → Box<dyn TradingSummaryProvider>
  intraday_provider() → Box<dyn IntradayProvider>
  orderbook_provider() → Box<dyn OrderBookProvider>
```

`ProviderKind::Stockbit` serves quotes (and, with a session, daily history):
fundamentals are `UNSUPPORTED`, the MSN-only capabilities say "requires
--provider msn", and `auto` history uses Yahoo unless a session covers the
window.

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
  `PROVIDERUNAVAILABLE`/`OFFLINE`/`AUTHERROR`/`UPSTREAMBLOCKED`, `5` `UNSUPPORTED`,
  `1` everything else. `UPSTREAMBLOCKED` is an upstream refusing this client
  outright (a CDN 403 page); retrying at once does not help.
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
