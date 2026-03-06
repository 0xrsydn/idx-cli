# idx-cli TODO

## ✅ Completed (v0.1.0)
- [x] Provider abstraction (MarketDataProvider trait, Yahoo impl)
- [x] `stocks quote <SYMBOL...>` — real-time quotes, multi-symbol
- [x] `stocks history <SYMBOL>` — historical OHLC data
- [x] File cache with TTL + offline mode + stale-cache fallback
- [x] Config system (TOML file, env vars, CLI flags, precedence)
- [x] Output modes: table (human) + JSON (agent)
- [x] 52-week range bar in quote table
- [x] `cache info` / `cache clear`
- [x] `config init` / `get` / `set` / `path`
- [x] Pre-commit hooks (fmt, clippy, test)
- [x] GitHub Actions CI
- [x] Nix flake + devshell
- [x] MIT license
- [x] crates.io metadata

## ✅ Completed (v0.1.1)
- [x] Technical analysis module (SMA, EMA, RSI, MACD, volume ratio)
- [x] Signal interpretation (bullish/bearish/neutral consensus)
- [x] `stocks technical <SYMBOL>` — full technical analysis command
- [x] Colored signal output (green/red/yellow)
- [x] 1-year lookback for SMA200 coverage

## ✅ Completed (v0.1.1 — fundamental suite)
- [x] `stocks growth <SYMBOL>` — revenue/earnings growth with signals
- [x] `stocks valuation <SYMBOL>` — PE, PB, ROE, margins, EV/EBITDA with signals
- [x] `stocks risk <SYMBOL>` — D/E, current ratio, ROA with signals
- [x] `stocks fundamental <SYMBOL>` — composite growth + valuation + risk
- [x] `stocks compare <SYM1,SYM2,...>` — side-by-side multi-symbol comparison
- [x] `analysis/fundamental.rs` — fundamental analysis module (ported from idx-mcp)
- [x] Yahoo quoteSummary endpoint parser (/v10/finance/quoteSummary)
- [x] 41 tests passing (22 unit + 19 integration)

## 🚧 Next Up
- [ ] Fix Yahoo crumb auth for quoteSummary (fetch crumb from consent.yahoo.com, pass as cookie/param) — unblocks live fundamental commands

## 📋 Backlog (per SPEC.md)
- [ ] `market summary` — IHSG index, market breadth
- [ ] `market movers` — top gainers/losers/volume
- [ ] `market sectors` — sector performance overview
- [ ] `screen query "<EXPR>"` — filter stocks by expression
- [ ] `screen presets` / `screen run <PRESET>` — built-in screener presets
- [ ] `watchlist` commands — create, manage, live watch
- [ ] `alerts` system (v0.2+) — price alerts with daemon
- [ ] `completions <SHELL>` — shell completion generation
- [ ] CSV/TSV output formats
- [ ] Additional providers (Alpha Vantage, Twelve Data, IDX official)

## 🐛 Known Issues
- [ ] Yahoo Finance returns 429 from datacenter IPs occasionally
- [ ] SMA200 trend shows "Insufficient data" if Yahoo returns < 200 candles
- [ ] Yahoo quoteSummary returns 401 from datacenter IPs — needs crumb/cookie auth (fundamental/growth/valuation/risk/compare commands affected)
