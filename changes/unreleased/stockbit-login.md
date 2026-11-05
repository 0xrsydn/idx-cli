### New Features

- `idx stockbit login`, `status` and `logout` sign in with your own Stockbit account. On a desktop, idx opens a browser window on Stockbit's sign-in page; over SSH or with `--no-browser`, a page on 127.0.0.1 takes the session from your own browser. idx never sees your password, stores the session with mode 0600, and refreshes it under a lock so concurrent processes never spend it twice. (#74)
- `idx stocks orderbook` and `get_orderbook` show a stock's order book: the depth ladder, OHLC, value, trades and foreign flow, and whether the book is live. (#74)
- With a session, `--provider stockbit` quotes come from Stockbit's session API, and price history from 5 days to 1 year uses Stockbit's daily charts (`--history-provider stockbit`, or `auto` under the stockbit provider). If Stockbit refuses the session, `auto` falls back to Yahoo with a warning. (#74)
- MCP: `stockbit_login` starts a sign-in and answers at once, and `stockbit_status` reports it: `sign_in.state` (waiting, signed_in or failed), plus `sign_in.warning` while it waits, for example when Stockbit refuses the session. (#74)

### Notes

- New dependency in idx-core: `base64` 0.22. (#74)
- Google sign-in does not work in idx's browser window; sign in with your password there, or use the local page (`idx stockbit login --no-browser`). (#74)
