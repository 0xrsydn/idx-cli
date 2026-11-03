---
title: Stockbit
description: Today's session by the minute and quotes with best bid/offer from Stockbit's public page, plus the order book and live quotes with your own Stockbit login. Both opt-in.
---

[Stockbit](https://stockbit.com)'s public symbol page carries a quote, the
best bid and offer, and today's price for every minute of the session. idx
reads that page directly. It needs no Stockbit account, no login and no
helper tools. With your own Stockbit account, idx can also read the order
book and live quotes; see [With your Stockbit login](#with-your-stockbit-login).

```bash
idx stocks intraday BBCA                     # today's session, by the minute
idx stocks intraday ^JKSE                    # an index (or IHSG, LQ45)
idx --provider stockbit stocks quote BBCA    # quote with bid/offer
idx -o json stocks intraday BBCA | jq '.points | length'
```

## What you get

| | `stocks intraday` | `--provider stockbit stocks quote` |
| --- | --- | --- |
| Price, change, previous close, volume | yes | yes |
| Best bid and offer | while the market is open | while the market is open |
| Market status (`market_status`) | yes | yes |
| Price per minute (`points`) | yes | no |
| Market cap, 52-week range | no | no |

- **Delayed.** Stockbit's terms describe its market data as "a real-time
  snapshot, delayed at least 10 minutes". It is not a live feed.
- **Bid and offer only while trading.** Outside the session, Stockbit still
  shows the book as it closed. idx leaves `bid` and `offer` out then, so the
  closing book never passes for a live one.
- **Top of the book only.** One bid and one offer price, no depth. The full
  order book needs your [Stockbit login](#with-your-stockbit-login).
- **Prices only.** The page has no per-minute volume or OHLC.
- **Empty sessions are data.** A suspended or delisted stock answers with
  `points: []` and a `session_note` saying so (`Stockbit lists the stock as
  suspended`). An active stock with no volume says `no trades this session`,
  even when its chart is a flat line at the previous close. Neither is an
  error.

`--provider stockbit` (or `IDX_PROVIDER=stockbit`, or
`idx config set general.provider stockbit`) changes only where quotes come
from. Without a login, history still comes from Yahoo. Company data,
fundamentals and screening answer `UNSUPPORTED` under it, so use MSN for
those.

## How idx talks to Stockbit

- One request per symbol to `https://stockbit.com/symbol/<CODE>`, at least one
  second apart across all your idx processes (a shell loop of `idx` calls
  included), cached for 5 seconds.
- An honest user agent, `idx-cli/<version> (+https://github.com/0xrsydn/idx-cli)`,
  not a browser disguise. Stockbit's CDN refuses some generic agents
  (`curl/…`) outright but lets this one through. No curl-impersonate
  needed.
- No retries. If Stockbit's CDN refuses your machine, the command fails at
  once with `UPSTREAMBLOCKED` (exit `4`):

  ```json
  {"error":true,"code":"UPSTREAMBLOCKED","message":"upstream blocked: stockbit.com refused this client (HTTP 403 from its CDN); ..."}
  ```

  Wait, or try from another network. `--provider msn` serves quotes without
  Stockbit.

## With your Stockbit login

Opt-in, and only with your own account. idx then talks to Stockbit's own API
(`exodus.stockbit.com`) with your session's tokens.

```bash
idx stockbit login                    # paste the credentialStorage cookie
idx stockbit status                   # logged in? when do the tokens expire?
idx stocks orderbook BBCA             # depth, queued orders, foreign flow
idx stocks orderbook BBRI --depth 5
idx --provider stockbit stocks quote BBCA                    # live, not delayed
idx --provider stockbit stocks history BBCA --period 1y      # Stockbit's daily candles
idx stockbit logout
```

**Logging in.** `idx stockbit login` opens Chrome (or Chromium, Edge, Brave,
Helium) on Stockbit's own sign-in page, in a browser profile only idx uses. Sign in
there as usual, including any one-time code: idx reads the
session cookie over the DevTools protocol, checks it with one request, and
closes the window. Your password never reaches idx. The profile sits beside
`config.toml`; idx takes the session cookie out of it once the session is
stored, so the profile keeps only Stockbit's device cookies. The profile
stays with the browser that made it: idx opens the same one next time, unless
`IDX_STOCKBIT_BROWSER` names another. Choose your browser that way on the
first sign-in if you have several.

Use your Stockbit password in that window. Google can refuse to sign in there
with "This browser or app may not be secure", most likely because idx starts
the browser with remote debugging on. If you only sign in with Google, use the
local page below: sign in from a private window, paste the cookie, then close
that window.

Run `login` again while the session is still good and there is nothing to
do: idx checks it with one request and exits without opening a browser. The
window it does open waits 15 minutes, and closes at once if you close it
yourself.

**How long a session lasts.** Stockbit's refresh token is good for about a
week, and idx spends it on every refresh, which extends the session. Use idx
at least once a week and you stay signed in. A longer gap, a `logout`, or
another tool spending the same token means one more sign-in.

Over SSH, on a machine with no display, without such a browser, or with
`--no-browser`, idx serves a small page on your own machine (127.0.0.1, port
7878 when it is free) instead. With
stockbit.com open and logged in, click the page's **Send my Stockbit
session** bookmarklet: it reads Stockbit's `credentialStorage` cookie in your
browser and hands it to the page, which checks it with one request and says
"Connected". If bookmarklets are blocked, click **Paste from clipboard**
after copying the cookie, or paste it into the box. `--port` picks another
port. Piping the value in still works
(`pbpaste | idx stockbit login`), as does a refresh token on its own. On a
server, set `IDX_STOCKBIT_REFRESH_TOKEN`. Set `IDX_STOCKBIT_BROWSER` to use a
browser idx does not find on its own.

The session lands in `~/.config/idx/stockbit-session.json`, readable only by
you. On macOS without `XDG_CONFIG_HOME` that is
`~/Library/Application Support/idx/` instead; idx finds a session in either
one, so the CLI and an MCP client started from the Dock share it. Tokens never appear in output, logs or errors. The page takes only the
session value, keeps it in memory, and closes when it is done (or after five
minutes). A session Stockbit refuses leaves a previously stored one in place.

## Remote and headless

The page listens on this machine's loopback interface only, and the browser
handoff needs a browser on this machine too. On a server, in a container, or
in a WSL distro, `127.0.0.1` is a different machine and neither will load.
Three ways around that, in order of how little they ask of you.

**Pipe the cookie over SSH.** No tunnel, one command. Copy the
`credentialStorage` value on the machine with the browser (DevTools,
Application, Cookies), then:

```bash
pbpaste | ssh <host> "idx stockbit login"
```

idx reads stdin, verifies the session, and stores it on the server. The value
never reaches a shell history or a file.

**Forward the page.** Run the page on the server and forward its port from the
machine with the browser:

```bash
idx stockbit login                    # on the server: over SSH, it serves the page
ssh -L 7878:127.0.0.1:7878 <host>     # where the browser is, keep it open
```

Then open the printed URL there and click the bookmarklet. Over SSH, idx
notices `SSH_CONNECTION`, skips the browser, and prints the exact forward
itself.

**Seed a token.** For anything unattended, keep the refresh token in a secret
store and pass it at run time:

```bash
op run --env-file=stockbit.env -- idx stocks orderbook BBCA
```

where the file holds `IDX_STOCKBIT_REFRESH_TOKEN=op://Private/stockbit/refresh`.
Only the refresh token is needed; idx mints its own access token.

**Agents.** Over MCP, an agent calls `stockbit_login`, which answers at once
and does the same as `idx stockbit login`. Where idx-mcp runs on your desktop,
a browser window opens on Stockbit's sign-in page (`"method": "browser"`) and
you sign in there. Over SSH or without a display, the answer carries the page
URL and its `ssh -L` forward (`"method": "page"`) for the agent to hand you.
The agent then polls `stockbit_status` until `sign_in.state` is `signed_in`
or `failed`. A session that already works answers `signed_in` without opening
anything, a second call while one is waiting returns it again, and
`{"method": "page"}` or `{"method": "browser"}` picks the way. If the server
stops while the window waits, the window closes with it.

One rule for all three: only one machine may own a session. Stockbit retires
the refresh token on every refresh, so a token copied to two machines signs the
other one out. Two machines need two sign-ins.

**One refresher per session.** Stockbit replaces the refresh token every time
it is used. Once idx refreshes your session, a browser holding the same
session has to log in again, and two tools sharing one session log each
other out. `idx stockbit login` keeps its own browser profile, so this only
affects a browser you use yourself. idx processes on one machine take turns
through a lock, so they never spend the token twice.

What the login adds:

| | Without a login | With your login |
| --- | --- | --- |
| `--provider stockbit` quotes | public page, delayed | Stockbit's API, live |
| Bid and offer in quotes | best price, while open | best price, while open |
| `stocks orderbook` | `AUTHERROR` | every level with lots and queued orders, OHLC, value, trades; foreign buy/sell/net once Stockbit marks them current |
| `stocks history` under `--provider stockbit` | Yahoo | Stockbit's daily candles for `5d` to `1y`; Yahoo for other windows |
| `stocks intraday` | public page | public page |

The order book is what Stockbit's own app shows. Outside trading it is the
book as the market left it, and the output says so. Foreign buy, sell and net
appear only when Stockbit marks them current: during trading its figures are
the previous session's, so idx leaves them out (`stocks flow` has foreign
flow session by session). Sizes are lots in the
table and shares in JSON. `--history-provider stockbit` asks for Stockbit's
candles explicitly; windows it charts only weekly or hourly (`2y`, `5y`,
`1wk`) answer `UNSUPPORTED`.

## Terms

Stockbit's [terms](https://stockbit.com/terms) allow its content for personal,
non-commercial use. They prohibit data mining, robots and redistribution
without Stockbit's written consent, which includes serving the data to other
people through `idx-mcp`. idx is not affiliated with or endorsed by
Stockbit. Use this for looking at a stock now and then, not for crawling the
market or republishing prices. With your login, automated use is tied to your
account: keep it human-paced.

Over MCP, the same data comes from `get_intraday`, `get_orderbook` and
`stockbit_status`, and from `get_quotes` and `get_price_history` when the
server runs with `IDX_PROVIDER=stockbit`. The server never logs in; run
`idx stockbit login` in a terminal.
