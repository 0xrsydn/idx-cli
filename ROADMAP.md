# Roadmap

Open work only. Shipped behavior is described in `README.md` and `docs/`;
history lives in git.

## Stocks
- `market summary`: market breadth and an index overview (index levels alone
  are already `stocks intraday ^JKSE` and `--provider stockbit stocks quote IHSG`)
- `market movers`: top gainers, losers, and volume
- `market sectors`: sector performance overview
- `screen query "<EXPR>"`: filter stocks by expression
- `screen presets` / `screen run <PRESET>`: built-in screener presets
  (graduate `screen` out of `stocks` only if these grow into a richer workflow)
- Multi-period financial statements (today `financials` is single-period)
- Order book depth without a login: no free source exists (IDX sells delayed
  depth to vendors); `stocks orderbook` needs the user's Stockbit login.
- Stockbit login follow-ups: a browser capture for `idx stockbit login`
  (Chrome DevTools, as fzn0x/idx does) instead of pasting the cookie,
  intraday OHLCV from the session's `1d` chart, and observing the session
  API's open-market state name (assumed `STATE_NAME_MARKET_OPEN`).
- Observe Stockbit's `market_hour.status` in the pre-open (08:45-09:00) and
  the closing auction (15:50-16:00) before `--watch` or alerts build on it;
  `open`, `break` and `close` are known, and the bid/offer gate hides the book
  for anything but `open`.
- Bandarmology follow-ups: a daily series (per-session readings and the
  cumulative top-N net) and a price-move phase (quiet accumulation, markup),
  calibration of the `top_concentration_v1` thresholds across more sessions,
  and a market-wide foreign flow screen from one IDX trading summary
- News beyond MSN in `news` / `get_company` (`docs/IDX_NEWS.md`): Google News
  RSS per ticker, and IDX disclosures and exchange news through a
  maintainer-published feed

## Workflow
- `watchlist`: named symbol sets usable by other commands
- `alerts`: price alerts with a daemon
- CSV/TSV output
- Additional providers (Alpha Vantage, Twelve Data); IDX's own site serves
  only `stocks flow --source idx` today

## Agent integration
- JSON Schema export for command output
- Composite `analyze` command (quote, fundamentals, ownership, sentiment)

## Known limits
- `stocks broker` and `stocks bandar` show only IPOT's top 10 buyers and
  sellers, ranked by gross lots, with figures of a million and more rounded
  to one decimal.
- `stocks flow --source idx` depends on IDX's Cloudflare passing a
  curl-impersonate profile (only `curl_safari184` in October 2026) and on the
  network; the default IPOT source rounds to one decimal.
- Stockbit's terms call its data delayed at least 10 minutes and for
  personal, non-commercial use (on 2026-10-23 the last trade was within a
  minute of the clock); its CDN refuses some user agents with a 403
  (`UPSTREAMBLOCKED`). Its order-book sizes are not shown: the page does not
  say whether they are lots or shares.
- Yahoo can return `429` from datacenter IPs; requests retry with backoff but
  can still fail.
- `stocks technical` reports the SMA200 trend as unavailable when fewer than
  200 daily candles are returned.
- MSN's `low-pe` screen returns no priced stocks (`PARSEERROR`; still so on
  2026-10-23, upstream; other filters work).
- MSN history for IDX is price-only (`1mo`, `3mo`, `1y`); Yahoo stays the
  default OHLCV source.
