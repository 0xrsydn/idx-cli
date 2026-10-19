---
title: Stocks
description: Quotes, history, company data, broker summaries, analysis and screening.
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
| `news` | recent news, this company's first (`ABOUT` = `company`), then general market news (`market`) |

## Broker summary

```bash
idx stocks broker BBCA
idx stocks broker BBCA --date 2026-10-16
idx stocks broker BBCA --from 2026-10-12 --to 2026-10-16 --board regular
```

Which brokers bought and sold a stock: the top 10 buyers and the top 10
sellers by lots, each with lots, value and average price, plus totals and the
net foreign value. Without dates it shows the latest session with trades.
`--investor foreign|domestic` counts only one side's clients (each broker row
is then that broker's foreign or domestic clients' trades, and the footer
gives what that side bought and sold), and `--board regular` leaves out
negotiated block trades.

The data comes from Indo Premier (IPOT) and is end of day: IDX hides broker
codes during trading. Buyers and sellers are ranked separately on gross
lots, so a broker can appear on both sides. The source rounds figures of a
million and more to one decimal, and the output says so.

## Foreign flow

```bash
idx stocks flow BBCA
idx stocks flow BBCA --days 20
idx stocks flow BBCA --from 2026-10-01 --to 2026-10-16
idx stocks flow BBCA --source idx
```

What foreign investors bought and sold, session by session, with the totals
over the range. Without dates it shows the latest 10 sessions (`--days`, at
most 60).

By default the figures come from Indo Premier (IPOT), like `broker`: foreign
buy, sell and net value across all boards and foreign investors' share of
trading. It needs nothing installed, takes two requests per session, and
rounds figures of a million and more to one decimal.

`--source idx` reads IDX's own daily trading summary instead: exact foreign
share counts, the close and day change, and the share of value traded
outside the regular board. Those figures are the regular board, and values
are estimated at each session's average price. Each session is one request
to idx.co.id, cached for every stock, and needs
[curl-impersonate](/start/install/) and a network IDX's Cloudflare lets
through; even then it can answer 403.

## Bandarmology

```bash
idx stocks bandar BBCA
idx stocks bandar BBCA --from 2026-10-12 --to 2026-10-16 --top 3
idx stocks screen --filter high-volume | jq -r '.[].symbol' | idx stocks bandar -
```

`bandar` reads the broker summary: for the top 1, 3, 5 and 10 brokers on each
side, the buyers' value minus the sellers' value as a share of the value
traded. The headline tier (`--top`, default 5) gives the signal:

| Top-N buy minus sell, % of value | Signal |
| --- | --- |
| +15% or more | big accumulation |
| +5% to +15% | accumulation |
| within ±5% | neutral |
| -15% to -5% | distribution |
| -15% or less | big distribution |

Several symbols give a screen ranked by that figure (at most 100, one request
each), and it takes the same date and filter flags as `broker`. With
`--investor foreign` (or `domestic`) it reads that side alone: the top buyers'
share of foreign buying against the top sellers' share of foreign selling.
That needs `--board all`, since IPOT gives one side's selling only across all
boards. It is a
heuristic: a broker trades for many clients, and a broker on both sides of
the summary is listed, since its gross figures overstate which way it traded.
The JSON names the rules in `method` (`top_concentration_v1`).

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
price history. Indo Premier (IPOT) serves broker summaries and IDX's own site
serves foreign flow, whichever provider is selected. Switch with `IDX_PROVIDER=msn|yahoo` and
`IDX_HISTORY_PROVIDER=auto|yahoo|msn`, or the matching
[config keys](/start/config/).
