---
title: Stocks
description: Quotes, history, company data, analysis and screening.
---

All `stocks` commands take IDX tickers (`BBCA`, or `BBCA.JK`). Every flag is
listed in the [command reference](/cli/reference/).

## Quotes and history

```bash
idx stocks quote BBCA BBRI,BMRI
idx stocks history BBCA --period 1y --interval 1wk
```

`quote` takes several tickers, separated by spaces or commas, or `-` to read
them from stdin. `history` takes `--period` (`1d` to `5y`) and `--interval`
(`1d`, `1wk`, `1mo`).

## Company data

| Command | What it shows |
| --- | --- |
| `profile` | sector, industry, description, contacts and officers |
| `financials` | income, balance sheet and cash flow statements (`--statement income,balance`) |
| `earnings` | reported and estimated earnings |
| `sentiment` | crowd sentiment |
| `insights` | AI insights from the provider |
| `news` | recent news |

## Analysis

| Command | What it computes |
| --- | --- |
| `technical` | trend, RSI, MACD, moving averages and signals |
| `growth` | revenue and earnings growth rating |
| `valuation` | P/E, P/B, ROE, net margin and EV/EBITDA, with a rating |
| `risk` | debt to equity, current ratio and ROA, with a rating |
| `fundamental` | growth, valuation and risk in one report |
| `compare` | `fundamental` for several tickers side by side |

```bash
idx stocks fundamental BBRI
idx stocks compare BBCA BBRI BMRI
```

## Screening

```bash
idx stocks screen --filter high-dividend --limit 20
```

Filters: `top-performers` (default), `worst-performers`, `high-dividend`,
`low-pe`, `52w-high`, `52w-low`, `high-volume`, `large-cap`.

Screen results pipe straight into `compare`:

```bash
idx stocks screen --filter high-dividend | jq -r '.[].symbol' | idx stocks compare -
```

## Providers

MSN is the default for quotes, company data and screening; Yahoo serves
price history. Switch with `IDX_PROVIDER=msn|yahoo` and
`IDX_HISTORY_PROVIDER=auto|yahoo|msn`, or the matching
[config keys](/start/config/).
