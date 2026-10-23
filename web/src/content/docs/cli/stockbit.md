---
title: Stockbit
description: Today's session by the minute and quotes with best bid/offer, from Stockbit's public page. Opt-in, no account.
---

[Stockbit](https://stockbit.com)'s public symbol page carries a quote, the
best bid and offer, and today's price for every minute of the session. idx
reads that page directly. It needs no Stockbit account, no login and no
helper tools.

```bash
idx stocks intraday BBCA                     # today's session, by the minute
idx stocks intraday ^JKSE                    # an index (or IHSG, LQ45)
idx --provider stockbit stocks quote BBCA    # quote with bid/offer
idx -o json stocks intraday BBCA | jq '.points | length'
```

## What you get

| | `stocks intraday` | `--provider stockbit stocks quote` |
| --- | --- | --- |
| Price, change, previous close, volume | yes | yes |
| Best bid and offer | while the market is open | while the market is open |
| Market status (`market_status`) | yes | yes |
| Price per minute (`points`) | yes | no |
| Market cap, 52-week range | no | no |

- **Delayed.** Stockbit's terms describe its market data as "a real-time
  snapshot, delayed at least 10 minutes". It is not a live feed.
- **Bid and offer only while trading.** Outside the session, Stockbit still
  shows the book as it closed. idx leaves `bid` and `offer` out then, so the
  closing book never passes for a live one.
- **Top of the book only.** One bid and one offer price, no depth. The full
  order book needs a Stockbit login, which idx does not do.
- **Prices only.** The page has no per-minute volume or OHLC.
- **Empty sessions are data.** A suspended or delisted stock answers with
  `points: []` and a `session_note` saying so (`Stockbit lists the stock as
  suspended`). An active stock with no volume says `no trades this session`,
  even when its chart is a flat line at the previous close. Neither is an
  error.

`--provider stockbit` (or `IDX_PROVIDER=stockbit`, or
`idx config set general.provider stockbit`) changes only where quotes come
from. History still comes from Yahoo. Company data, fundamentals and
screening answer `UNSUPPORTED` under it, so use MSN for those.

## How idx talks to Stockbit

- One request per symbol to `https://stockbit.com/symbol/<CODE>`, at least one
  second apart across all your idx processes (a shell loop of `idx` calls
  included), cached for 5 seconds.
- An honest user agent, `idx-cli/<version> (+https://github.com/0xrsydn/idx-cli)`,
  not a browser disguise. Stockbit's CDN refuses some generic agents
  (`curl/…`) outright but lets this one through. No curl-impersonate
  needed.
- No retries. If Stockbit's CDN refuses your machine, the command fails at
  once with `UPSTREAMBLOCKED` (exit `4`):

  ```json
  {"error":true,"code":"UPSTREAMBLOCKED","message":"upstream blocked: stockbit.com refused this client (HTTP 403 from its CDN); ..."}
  ```

  Wait, or try from another network. `--provider msn` serves quotes without
  Stockbit.

## Terms

Stockbit's [terms](https://stockbit.com/terms) allow its content for personal,
non-commercial use. They prohibit data mining, robots and redistribution
without Stockbit's written consent. idx is not affiliated with or endorsed by
Stockbit. Use this for looking at a stock now and then, not for crawling the
market or republishing prices.

Over MCP, the same data comes from the `get_intraday` tool, and from
`get_quotes` when the server runs with `IDX_PROVIDER=stockbit`.
