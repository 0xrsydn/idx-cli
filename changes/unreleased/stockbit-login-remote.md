### New Features

- `idx stockbit login` takes the `credentialStorage` cookie pasted in the terminal while its local page waits, so signing in from a remote terminal needs no port forward.

### Bug Fixes

- `idx stockbit login` on a machine with no display now says how to reach its page from another machine (the `ssh -L` forward, or the paste), which it only did over SSH.
- The `ssh -L` forward that `idx stockbit login` and `stockbit_login` suggest names this host when `HOSTNAME` is not exported, where it said `<this host>`.
- `stocks actions SYM --kind ipo` and `get_corporate_actions` with `ipo` list only that stock's IPO, where they listed the market's recent IPOs under its name.
- `ownership movements` and `get_shareholder_movements` refuse a `from`..`to` range over 365 days with `INVALIDINPUT`, where Stockbit's `HTTP 400: INVALID_PARAMETER` came through.
- `ownership movements --to` with a date before Stockbit's feed begins returns no rows, where it returned one row from years after that date.
- `ownership movements` says `symbol not found` for a code Stockbit does not know, where it printed an empty list.
- Warrants and rights (`INET-W`, `BMAS-R`) work in `stocks orderbook`, `stocks intraday` and Stockbit quotes and history, where they answered `UNSUPPORTED: Stockbit covers IDX (.JK) symbols only`.
- Index tickers work on Yahoo: `--provider yahoo stocks quote ^JKSE`, `stocks history ^JKSE` and `stocks technical ^JKSE` answered `symbol not found: ^JKSE.JK`.
- A Yahoo quote takes the day's change from Yahoo's own percentage when its previous close is stale, as it was for `^JKSE` (+4.48% shown on a +1.78% day).

### Improvements

- `stocks calendar` and `get_corporate_calendar` warn when the range starts before Stockbit's market-wide lists do, naming each kind and the date its list begins, where a past range silently came back short.
- A Stockbit session file that other users can read is set back to owner-only, and a damaged one is named in a warning, where it read as "not logged in" with no reason.
- `stocks orderbook` and `get_orderbook` warn when the market is not in a trading session (closed, the break, an auction, the negotiated market), since the book they return then looks like a live one.
- `stocks quote ^JKSE` works on the default provider: MSN has no indices, so an index ticker asked of it is quoted by Yahoo (`source` says `yahoo`), where it answered `symbol not found`.

### Output Changes

- `stockbit_login` and `stockbit_status` answer `"remote": true` for a page sign-in on a machine with no display, as they did over SSH.
- `ownership movements` and `get_shareholder_movements` answer `SYMBOLNOTFOUND` (exit 3) for an unknown symbol, where they answered an empty list.
- `stocks orderbook` prints a `warning` line on stderr outside a trading session (a JSON line with `-o json`), and `get_orderbook` carries it in `warnings`; stdout and the exit code are unchanged.

### Docs

- The ownership and Stockbit pages state the 365-day limit of `ownership movements` and show a warrant ticker.
