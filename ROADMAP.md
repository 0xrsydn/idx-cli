# Roadmap

Open work only. Shipped behavior is described in `README.md` and `docs/`;
history lives in git.

## Stocks
- `market summary`: IHSG index and market breadth
- `market movers`: top gainers, losers, and volume
- `market sectors`: sector performance overview
- `screen query "<EXPR>"`: filter stocks by expression
- `screen presets` / `screen run <PRESET>`: built-in screener presets
  (graduate `screen` out of `stocks` only if these grow into a richer workflow)
- Multi-period financial statements (today `financials` is single-period)

## Workflow
- `watchlist`: named symbol sets usable by other commands
- `alerts`: price alerts with a daemon
- CSV/TSV and JSONL output
- Stdin symbol input for chaining (`... | idx stocks fundamental --stdin`)
- Additional providers (Alpha Vantage, Twelve Data, IDX official)

## Agent integration
- MCP server exposing the existing commands as tools (planned as `idx-mcp`)
- JSON Schema export for command output
- Composite `analyze` command (quote, fundamentals, ownership, sentiment)

## Known limits
- Yahoo can return `429` from datacenter IPs; requests retry with backoff but
  can still fail.
- `stocks technical` reports the SMA200 trend as unavailable when fewer than
  200 daily candles are returned.
- MSN history for IDX is price-only (`1mo`, `3mo`, `1y`); Yahoo stays the
  default OHLCV source.
