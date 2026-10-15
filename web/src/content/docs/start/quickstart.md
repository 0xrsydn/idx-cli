---
title: Quickstart
description: First stock and ownership commands.
---

## Stocks

Tickers are IDX codes; the `.JK` suffix is optional.

```bash
idx stocks quote BBCA                 # one quote
idx stocks quote BBCA BBRI BMRI       # several (spaces or commas)
idx stocks history BBCA --period 1y   # daily OHLCV
idx stocks technical BBCA             # trend, RSI, MACD, moving averages
idx stocks fundamental BBCA           # growth, valuation and risk together
idx stocks compare BBCA BBRI BMRI     # fundamentals side by side
idx stocks screen --filter high-dividend
```

## Ownership

Ownership data comes from the monthly KSEI register of holders above 1%.
Download the maintained snapshot once; after that every query runs offline.

```bash
idx ownership sync                    # a few MB, safe to rerun
idx ownership ticker BBCA             # who holds BBCA
idx ownership entity "ANTHONI SALIM"  # everything one investor holds
idx ownership search salim            # find investor names
idx ownership releases                # which months you have
idx ownership changes --from 2026-08-31 --to 2026-09-30
```

## JSON

In a terminal idx prints tables. When its output is piped or captured it
prints JSON, so scripts and agents need no flag:

```bash
idx stocks quote BBCA BBRI | jq -r '.[] | "\(.symbol) \(.price)"'
idx -o json stocks quote BBCA        # force JSON in a terminal
```

See [Output and exit codes](/cli/output/) for the full contract.

## Next

- [Stocks guide](/cli/stocks/) and [ownership guide](/cli/ownership/)
- [Configuration](/start/config/)
- [Use idx from an AI agent](/mcp/setup/)
