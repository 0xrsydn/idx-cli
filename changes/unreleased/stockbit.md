### New Features

- `idx stocks intraday` and the MCP tool `get_intraday` show today's session by the minute from Stockbit's public page, for stocks and the `IHSG` and `LQ45` indices: the quote, the best bid and offer while the market is open, and a sparkline. No login is needed. (#73)
- `--provider stockbit` (or `IDX_PROVIDER=stockbit`, or `general.provider`) serves quotes from Stockbit, with bid, offer and market status. History stays on Yahoo, and the MSN-only commands answer `UNSUPPORTED`. (#73)

### Output Changes

- New error code `UPSTREAMBLOCKED` (exit 4): a site's CDN refused idx, so the request is not retried. (#73)
- Under the stockbit provider, quotes carry `market_status` (`open` or `close`), `bid` and `offer` (while the market is open) and `session_note`. MSN and Yahoo output is unchanged. (#73, #74)
- `stocks intraday` marks a chart from before the latest session with `behind_session`, as quotes do. (#73)

### Notes

- Stockbit's terms allow personal, non-commercial use, and its data is delayed at least 10 minutes. idx sends at most one request a second across all its processes. See the [Stockbit page](https://idx.rasyidanaf.com/cli/stockbit/). (#73)
