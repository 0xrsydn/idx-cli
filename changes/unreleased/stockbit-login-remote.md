### New Features

- `idx stockbit login` takes the `credentialStorage` cookie pasted in the terminal while its local page waits, so signing in from a remote terminal needs no port forward. (#0)

### Bug Fixes

- `idx stockbit login` on a machine with no display now says how to reach its page from another machine (the `ssh -L` forward, or the paste), which it only did over SSH. (#0)
- The `ssh -L` forward that `idx stockbit login` and `stockbit_login` suggest names this host when `HOSTNAME` is not exported, where it said `<this host>`. (#0)
- `stocks actions SYM --kind ipo` and `get_corporate_actions` with `ipo` list only that stock's IPO, where they listed the market's recent IPOs under its name. (#0)
- `ownership movements` and `get_shareholder_movements` refuse a `from`..`to` range over 365 days with `INVALIDINPUT`, where Stockbit's `HTTP 400: INVALID_PARAMETER` came through. (#0)
- `ownership movements --to` with a date before Stockbit's feed begins returns no rows, where it returned one row from years after that date. (#0)
- `ownership movements` says `symbol not found` for a code Stockbit does not know, where it printed an empty list. (#0)
- Warrants and rights (`INET-W`, `BMAS-R`) work in `stocks orderbook`, `stocks intraday` and Stockbit quotes and history, where they answered `UNSUPPORTED: Stockbit covers IDX (.JK) symbols only`. (#0)
- Index tickers work on Yahoo: `--provider yahoo stocks quote ^JKSE`, `stocks history ^JKSE` and `stocks technical ^JKSE` answered `symbol not found: ^JKSE.JK`. (#0)
- A Yahoo quote takes the day's change from Yahoo's own percentage when its previous close is stale, as it was for `^JKSE` (+4.48% shown on a +1.78% day). (#0)

### Improvements

- `stocks calendar` and `get_corporate_calendar` warn when the range starts before Stockbit's market-wide lists do, naming each kind and the date its list begins, where a past range silently came back short. (#0)
- A Stockbit session file that other users can read is set back to owner-only, and a damaged one is named in a warning, where it read as "not logged in" with no reason. (#0)
- `stocks orderbook` and `get_orderbook` warn when the market is not in a trading session (closed, the break, an auction, the negotiated market), since the book they return then looks like a live one. (#0)

### Output Changes

- `stockbit_login` and `stockbit_status` answer `"remote": true` for a page sign-in on a machine with no display, as they did over SSH. (#0)
- `ownership movements` and `get_shareholder_movements` answer `SYMBOLNOTFOUND` (exit 3) for an unknown symbol, where they answered an empty list. (#0)
- `stocks orderbook` prints a `warning` line on stderr outside a trading session (a JSON line with `-o json`), and `get_orderbook` carries it in `warnings`; stdout and the exit code are unchanged. (#0)

### Docs

- The ownership and Stockbit pages state the 365-day limit of `ownership movements` and show a warrant ticker. (#0)
