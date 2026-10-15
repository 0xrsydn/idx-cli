---
name: idx
description: Indonesian stock market (IDX) quotes, fundamentals, technicals, screening, and KSEI shareholder ownership through the `idx` CLI. Use for questions about IDX-listed stocks (tickers like BBCA, BBRI, TLKM), who owns them, or ownership changes.
---

# idx

`idx` is installed as a CLI. Docs: https://idx.rasyidanaf.com (any page as Markdown: add `.md`).

## Rules

- Capture output: it is JSON whenever stdout is not a terminal. Parse it; don't scrape tables.
- On failure stdout is empty and stderr is `{"error":true,"code":"...","message":"..."}`.
  Exit `2` fix the input, `3` not found or no cached data, `4` provider unavailable (retry later), `5` unsupported.
- Tickers are IDX codes (`BBCA`); `.JK` is optional. `stocks quote` and `stocks compare` take several.
- Ownership data is local. If an ownership command says the database is empty, run `idx ownership sync` once.
- Ownership percentages are basis points: `5494` = 54.94%.

## Commands

```bash
idx stocks quote BBCA BBRI                  # price, change, volume, 52-week range
idx stocks history BBCA --period 1y         # OHLCV; --interval 1d|1wk|1mo
idx stocks technical BBCA                   # trend, RSI, MACD, moving averages
idx stocks fundamental BBCA                 # growth, valuation, risk ratings
idx stocks compare BBCA BBRI BMRI
idx stocks profile BBCA                     # also: financials, earnings, sentiment, insights, news
idx stocks screen --filter high-dividend    # top-performers, worst-performers, low-pe, 52w-high, 52w-low, high-volume, large-cap

idx ownership ticker BBCA                   # holders above 1%, concentration, free float
idx ownership entity "ANTHONI SALIM"        # one investor's holdings
idx ownership search salim                  # find investor names
idx ownership releases                      # available months, newest first
idx ownership changes --from 2026-08-31 --to 2026-09-30
```

Run `idx <command> --help` for every flag.
