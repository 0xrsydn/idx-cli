# Changelog

Notable changes to `idx` (idx-cli), `idx-mcp` and the `idx-core` library, newest
first. Versions follow [Semantic Versioning](https://semver.org); before 1.0, a
minor version (0.4.0, 0.5.0) adds features and a patch version (0.3.3) fixes
them. Each version's section is also its GitHub release notes.

Changes that are merged but not yet released live in
[`changes/unreleased/`](changes/unreleased/), one file per pull request, and are
folded in here at release time. See [`changes/README.md`](changes/README.md).

## [0.5.1] - 2026-11-12

Getting ownership reports from IDX when its Cloudflare challenges curl. idx no longer sends a fixed browser User-Agent over curl-impersonate's own, which drew most of the challenges. When a challenge still blocks curl, `ownership discover --list-file` finds a report in IDX's list saved from a browser, and `import --file --source-url` imports the downloaded file with its address. Scripts that read `query_url` from `ownership discover` JSON should note it is now IDX's list request.

### New Features

- `ownership import --file` takes `--source-url`, the address a report was downloaded from, and records it as the release's `source_url` as an import by `--url` would, so a report fetched in a browser when IDX refuses curl keeps its provenance. (#85)
- `ownership discover --list-file <json>` reads IDX's report list saved from a browser, contacting nothing, so when IDX refuses curl you can still find a report and import it with `import --file --source-url`. (#86)

### Bug Fixes

- `ownership discover`, `ownership import --url` and `stocks flow --source idx` no longer send a fixed browser User-Agent over every curl-impersonate profile, which did not match the profile's fingerprint and drew Cloudflare challenges from IDX; each profile now sends its own. (#87)

### Improvements

- `ownership discover` reads IDX's report list from the JSON its data page loads, one small request covering every year, instead of picking it out of the page's HTML, which breaks when IDX changes the page. (#86)

### Output Changes

- When `ownership import` skips a release that is already imported, its `source_url` is the one stored with that release, no longer the `--url` or `--source-url` just passed; `--force` re-imports it to record a new one. (#85)
- In `ownership discover` JSON, a data page report's `query_url` is now the list request (`https://www.idx.co.id/primary/list/id/perusahaan-tercatat/data-kepemilikan-saham?lang=id&start=0&length=9999`) instead of the page address; `listing_page_url` is still the page. A snapshot published from it records the same in `source.query_url`. (#86)

### Docs

- The website's home page, About page and Stockbit page now cover 0.5's Stockbit features (order book, corporate actions, insider trades, live quotes and daily history), list Stockbit and Indo Premier among the data sources, and count idx-mcp's 21 tools. (#88)

### Notes

- The test override `IDX_OWNERSHIP_DATA_PAGE_URL` is replaced by `IDX_OWNERSHIP_DATA_LIST_URL`, the address of the list. (#86)

**Full Changelog**: https://github.com/0xrsydn/idx-cli/compare/v0.5.0...v0.5.1

## [0.5.0] - 2026-11-09

Stockbit data: intraday and quotes without a login, and with your own Stockbit session the order book, daily history, corporate actions and shareholder movements. **Upgrade from 0.4.0** to import IDX's ownership register from October 2026 on. Scripts that retry on exit 4 for idx.co.id should also handle the new exit 6, a Cloudflare challenge.

### New Features

- `idx stocks intraday` and the MCP tool `get_intraday` show today's session by the minute from Stockbit's public page, for stocks and the `IHSG` and `LQ45` indices: the quote, the best bid and offer while the market is open, and a sparkline. No login is needed. (#73)
- `--provider stockbit` (or `IDX_PROVIDER=stockbit`, or `general.provider`) serves quotes from Stockbit, with bid, offer and market status. History stays on Yahoo, and the MSN-only commands answer `UNSUPPORTED`. (#73)
- `idx stockbit login`, `status` and `logout` sign in with your own Stockbit account. On a desktop, idx opens a browser window on Stockbit's sign-in page; over SSH or with `--no-browser`, a page on 127.0.0.1 takes the session from your own browser. idx never sees your password, stores the session with mode 0600, and refreshes it under a lock so concurrent processes never spend it twice. (#74)
- `idx stocks orderbook` and `get_orderbook` show a stock's order book: the depth ladder, OHLC, value, trades and foreign flow, and whether the book is live. (#74)
- With a session, `--provider stockbit` quotes come from Stockbit's session API, and price history from 5 days to 1 year uses Stockbit's daily charts (`--history-provider stockbit`, or `auto` under the stockbit provider). If Stockbit refuses the session, `auto` falls back to Yahoo with a warning. (#74)
- MCP: `stockbit_login` starts a sign-in and answers at once, and `stockbit_status` reports it: `sign_in.state` (waiting, signed_in or failed), plus `sign_in.warning` while it waits, for example when Stockbit refuses the session. (#74)
- `idx stocks actions` and `get_corporate_actions` list one stock's corporate actions from your Stockbit session, newest first: cash and stock dividends, bonus shares, rights issues, warrants, splits and reverse splits, shareholders' meetings, public exposes and tender offers. `--kind` picks kinds, and `--kind ipo` adds the stock's IPO. (#78)
- `idx stocks calendar` and `get_corporate_calendar` list the market's actions dated in a range, oldest first: by default today and the next 30 days, at most 366 days. Rights trading, tender offers and IPO offers still open in the range are listed too, so an offer that began earlier shows while you can act on it. (#78)
- An action's `date` is its cum date where there is an entitlement (dividends, bonuses, rights, splits), else the event's first day. Where Stockbit gives only an ex date, the table marks it `(ex)`. Dividends paid in another currency carry `currency`. (#78)
- `idx ownership movements` and `get_shareholder_movements` list reported changes in insiders' and major holders' stakes from your Stockbit session, newest first. `IDX` rows are insider disclosures with the trade price; `KSEI` rows are holders at 5% and above, with a price Stockbit estimates. Stockbit filters by date, holder, action and source, and `--pages` reads up to 10 pages of 50 rows. If a later page fails, you get the rows read before it, with `incomplete` saying why. (#80)
- `ownership discover --require-data-page` fails when the IDX data page can't be read, instead of warning and listing older announcements. The ownership publisher uses it. (#84)

### Bug Fixes

- `ownership import` reads IDX's above-1% register from October 2026 on, which adds a `CONTROLLER_OR_AFFILIATE` column; 0.4.0 rejects these files with `PARSEERROR`. Any other unknown column is still rejected. (#79)
- The ownership publisher no longer reports `RESULT: up-to-date` when only the IDX data page failed and the newest report was missing; it rebuilds and fails loudly instead. (#84)

### Improvements

- CI and the release build macOS binaries on `macos-15` runners, since GitHub retired `macos-14`. (#75)
- idx reads Stockbit's session API only on the endpoints it uses. The same API places orders and withdraws cash; any other path fails inside idx before a request leaves your machine. (#78)
- The `ownership` commands honour `--offline` and `--no-cache`; only `movements` uses them. (#80)
- The stale-cache warning names the failure that caused it, for example `network failed (provider unavailable), serving stale cache for BBCA.JK`. (#81)
- A Cloudflare challenge from idx.co.id ends after one round of browser profiles instead of two, since no profile can solve it, and `-v` logs which profile fetched each IDX URL. (#84)

### Output Changes

- New error code `UPSTREAMBLOCKED` (exit 4): a site's CDN refused idx, so the request is not retried. (#73)
- Under the stockbit provider, quotes carry `market_status` (`open` or `close`), `bid` and `offer` (while the market is open) and `session_note`. MSN and Yahoo output is unchanged. (#73, #74)
- `stocks intraday` marks a chart from before the latest session with `behind_session`, as quotes do. (#73)
- MCP answers and errors carry `warnings` when idx worked around something the CLI warns about on stderr: history that fell back to Yahoo, a stale cache entry and the failure behind it, or a shareholder movements query cut short. (#81)
- **Breaking:** a Cloudflare challenge from idx.co.id now fails with the new error code `CLOUDFLARECHALLENGE` and exit status 6, instead of `HTTP` and exit 4. This affects `ownership discover`, `ownership import --url` and `stocks flow --source idx`; a script that checks for exactly 4 needs updating. (#84)
- The ownership publisher ends a challenged run with `RESULT: blocked stage=<stage>` and exit 6; every other failure stays `RESULT: FAILED`. (#84)

### Docs

- `CHANGELOG.md` records every release since 0.2.0, and each release's GitHub notes come from it. (#82)

### Notes

- Stockbit's terms allow personal, non-commercial use, and its data is delayed at least 10 minutes. idx sends at most one request a second across all its processes. See the [Stockbit page](https://idx.rasyidanaf.com/cli/stockbit/). (#73)
- New dependency in idx-core: `base64` 0.22. (#74)
- Google sign-in does not work in idx's browser window; sign in with your password there, or use the local page (`idx stockbit login --no-browser`). (#74)

**Full Changelog**: https://github.com/0xrsydn/idx-cli/compare/v0.4.0...v0.5.0

## [0.4.0] - 2026-10-19

Broker summaries, bandarmology and foreign flow, from public data with no login or key.

### New Features
- `idx stocks broker` and the MCP tool `get_broker_summary` show a stock's top 10 buying and selling brokers over a day or a date range, with totals. You can filter by investor side and board. The data comes from Indo Premier (IPOT). (#62, #63)
- `idx stocks bandar` and `analyze_bandar` read broker concentration as buy minus sell value for the top 1, 3, 5 and 10 brokers, and give a signal from big accumulation to big distribution. Several symbols make a ranked screen that `stocks screen` can pipe into. The thresholds are heuristics. (#64)
- `idx stocks flow` and `get_foreign_flow` show foreign buying, selling and net value session by session. The data comes from IPOT by default; `--source idx` uses IDX's trading summary for exact share counts. (#65, #68)
- `--investor foreign|domestic` now reads one investor side in `broker` and `bandar`, with that side's buy and sell totals. (#67)

### Bug Fixes
- Quotes and IDX screen rows now carry `behind_session`: true when the last trade is from before the latest IDX session. The quote table adds a note for these quotes, so a provider still showing the previous close after the session is no longer passed off as the latest price. (#71, #50)

### Improvements
- IPOT broker pages are parsed with html5ever and accepted only in IPOT's exact markup. A page that differs is rejected with an error and never misread. (#70)
- IPOT pages for dates other than those requested are rejected before caching, and so are IDX trading summaries with no rows for the requested session. (#70)
- Bandar readings use checked arithmetic, and the table says "No trades." only when no broker traded. (#70)
- Flow and broker data share caches. Past sessions are cached for days and today for 5 minutes, and offline `latest` lookups skip uncached days. (#62, #65, #68)
- The `analyze_bandar` `top` parameter lists its allowed values (1, 3, 5, 10) in the MCP schema. (#70)

### Notes
- `stocks flow --source idx` needs curl-impersonate. The default IPOT source does not.
- New dependency: `scraper` 0.27 (html5ever). Servo's `selectors`, `cssparser` and `dtoa-short` are MPL-2.0 and used unmodified.
- JSON output for quotes and screens adds the `behind_session` field; no existing field changed.

**Full Changelog**: https://github.com/0xrsydn/idx-cli/compare/v0.3.3...v0.4.0

## [0.3.3] - 2026-10-18

**Upgrade from 0.3.2**: ownership `changes` could report the wrong direction for investors with several accounts, and invalid dates were answered instead of rejected.

### Bug Fixes

- `ownership changes` and the `get_ownership_changes` tool compare an investor's total stake per ticker: KREN's KRESNA PRIMA INVEST, which added a second account, was reported as a decrease (944 → 401 bps) instead of an increase (944 → 1345). A corrected KSEI file for a month now replaces the original instead of mixing with it, and a file carrying several report dates compares each date on its own rows. (#53)
- `get_ownership_changes` rejects dates that are not imported releases, are reversed, or are malformed; an empty `from` used to report every holder as new. (#53)
- Investor search finds aliases, the raw names KSEI prints: `PT ASTRA INTERNATIONAL TBK` now finds ASTRA INTERNATIONAL. An empty investor name is an error instead of an arbitrary match. (#53)
- `ownership graph` reaches every connected holder (holders whose id was a prefix of another's were dropped) and no longer slows down exponentially with depth. (#53)
- `ownership sync` replaces the database in a single step, so an interrupted sync can no longer leave it missing; a snapshot whose holdings are missing is rejected before it replaces good data. A backup left by an interrupted older sync is restored, not overwritten. (#54)
- Requests to MSN and Yahoo give up after 30 seconds instead of hanging, and ownership commands wait at most 30 seconds for another process's lock, so a stalled call cannot block every MCP tool. Downloads stop at 256 MiB as they arrive, including compressed ones. (#55)
- Escape sequences in provider text (news titles, insights, errors, names) are neutralized before reaching the terminal. Descriptions and URLs with non-ASCII characters no longer crash table output. (#56)
- In JSON mode, argument errors and warnings are JSON, so stderr stays parseable. `idx config` honors `-o json`; piped output stays plain text. (#56)
- A "healthy" fundamental rating needs valuation and risk data; negative equity counts as high risk. (#57)
- `install.sh` installs the newest release even when the releases API answers on one line. (#58)
- `cache clear` removes symlinks without following them out of the cache. (#56)

### Improvements

- `zip` moves off a yanked release, to 9.0.4. (#52, #51)
- Price data outside any plausible range is treated as missing instead of overflowing. (#55)
- The snapshot publisher handles report titles with line breaks, checks its manifest before upload, and copies its database safely. (#58)
- Release workflow actions are pinned to commit SHAs, and only the publishing job can write to releases. (#58)
- Help text covers config keys and output, `ownership changes` date rules, and alias search. (#56)

### Testing

- Every fix has a regression test that fails on 0.3.2. The agent smoke test passes in Claude Code, Codex, pi, Hermes Agent, and opencode 1 and 2 through `npx -y idx-mcp`, ten cases each. Found by a full-repo audit with Codex (gpt-6.1-sol, xhigh) and re-reviewed until clean; Pullfrog's PR review found four more, fixed here.

**Full Changelog**: https://github.com/0xrsydn/idx-cli/compare/v0.3.2...v0.3.3

## [0.3.2] - 2026-10-16

**Upgrade from 0.3.1** if you run idx-mcp through `npx`, Hermes Agent, pnpm, or bun: 0.3.1 does not start there on npm 12.

### New Features

- News items carry `relevance`: `company` items about this ticker come first, newest first, then `market` items with general market news. The `stocks news` table gains an `ABOUT` column, and the `get_company` news section returns the same labels. (#47)
- `get_ownership_changes` pages through large diffs with `offset` / `next_offset` and returns a `summary` over every row (counts by change type and the tickers with the most changes), so agents can answer overview questions without fetching every row. Rows are ordered by largest change first. (#48)

### Bug Fixes

- `npx -y idx-mcp` and `npx idx-cli` work on npm 12, pnpm 10+, and bun, which skip install scripts: the launcher now downloads the binary on first run instead of exiting with `installed binary not found`, which MCP clients reported as "Connection closed". This affected every Hermes Agent user. (#41, #42)
- A first run with a read-only package directory, such as a root-owned `sudo npm install -g` on npm 12, installs the binary to a per-user cache instead of failing with `EACCES`. (#46)
- Stopping the launcher during the first-run download (`SIGTERM`, `SIGHUP`, or `SIGINT` sent to its PID) stops the installer, removes the partial file, and exits by the same signal. (#46)
- A local `IDX_BINARY_URL` binary is verified against `IDX_BINARY_SHA256` after it is copied, so the installed bytes are the verified ones. (#46)
- The user-cache fallback runs only from an absolute cache root, so an invalid `XDG_CACHE_HOME` or an empty `HOME` can never make the launcher run a binary from the current directory. (#46)
- News is ordered by publish time rather than by timestamp text, so fractional seconds and time-zone offsets no longer move older items ahead. (#47)

### Improvements

- `get_ownership_changes` returns at most 300 rows per page (previously 2000, about 200 KB), which no longer overflows agents' tool output. The over-limit error points to `offset` and `summary`. (#48)
- News is fetched wider (at least 50 items), so this company's articles further down MSN's feed are found. MSN's stock tags are combined with the ticker and company name to find them. (#47)

### Testing

- The agent smoke test runs Claude Code, Codex, pi, Hermes Agent, and opencode 1 and 2 against `npx -y idx-mcp`, each from a cold npm cache in an empty directory. Each agent must install the binary itself, and ten cases call every MCP tool except `ownership_sync`, which needs `--allow-sync`. All six harnesses pass on this release. (#44)
- The npm smoke test covers installs without postinstall, read-only installs, and a planted binary that must never run. (#46)

**Full Changelog**: https://github.com/0xrsydn/idx-cli/compare/v0.3.1...v0.3.2

## [0.3.1] - 2026-10-15

Security and data-safety fixes for 0.3.0. **Upgrade from 0.3.0**; 0.3.0 is yanked on crates.io and deprecated on npm.

### Security
- **Path traversal via ticker symbols.** A ticker such as `../../x` was used directly in cache file names, so idx could read or delete `.json` files outside its cache directory. Through `idx-mcp`, a single tool call from a prompt-injected agent was enough. Tickers are now validated (`invalid ticker symbol`, exit 2) and cache file names are encoded.

### Ownership database
- A cross-process lock (`<db>.lock`) now guards the ownership database: sync installs, imports and entity edits take it exclusively, reads take it shared. Before, a sync could replace the database under an active import or reader and lose data.
- Sync downloads without holding the lock, and downloads now time out (15 s connect, 10 min total) instead of hanging.
- A symlinked `ownership.db_path` keeps its symlink: sync updates the real file and shares its lock.
- A rebuilt snapshot never replaces a local database that holds more releases than it does.

### MCP
- Input limits: at most 20 symbols per call (duplicates collapsed), caps on `limit`, at most 4 concurrent tool calls. The tool schemas state the limits.

### Robustness
- Atomic cache writes; per-client Yahoo cookie files.
- Size caps on ZIP entries and XLSX workbooks; two parser inputs that could crash or overflow now return errors.

### Other
- Minimum supported Rust version is now 1.89 (for std file locks).
- `tests/agents/agent-smoke.sh`: opt-in smoke test that drives real coding agents (Claude Code, Codex, pi, Hermes, opencode) against idx-mcp.

## [0.3.0] - 2026-10-15

### Highlights
- **New: `idx-mcp`, an MCP server.** It serves the same market, financial and ownership data to MCP clients over stdio, through 11 tools: `get_quotes`, `get_price_history`, `screen_stocks`, `get_company`, `analyze_stock`, `compare_stocks`, `get_shareholders`, `get_investor_holdings`, `search_investors`, `get_ownership_changes`, `ownership_status`.
  - Install with `claude mcp add idx -- npx -y idx-mcp`, `cargo install idx-mcp`, or `nix profile install github:0xrsydn/idx-cli#idx-mcp`.
  - Live results come as `{data, source, fetched_at, stale}`.
  - Errors use the CLI's `{error, code, message}` body.
  - The server shares idx's config, cache and ownership database.
  - `--allow-sync` adds an `ownership_sync` tool.
- **Agent-native CLI output.** idx now prints JSON when stdout is not a terminal, and tables when it is. Agents and scripts get structured data without flags. JSON is compact when piped. `-o jsonl` prints one record per line. No color codes appear when piped.
- **`idx-core` library.** All data access, caching and analysis now live in a separate crate shared by `idx` and `idx-mcp`.

### Breaking changes
- **Default output when piped is JSON.** Use `-o table`, `IDX_OUTPUT=table`, or `general.output = "table"` to get tables back. A config created by `idx config init` before 0.3 pins `output = "table"`; run `idx config set general.output auto` to switch to the new behavior.
- **Exit codes by error class.** Before 0.3, every error exited with 1.

  | Exit | Meaning |
  | --- | --- |
  | 2 | invalid input or config |
  | 3 | not found, no data, or no cached data offline |
  | 4 | upstream unavailable; retry later |
  | 5 | unsupported |
  | 1 | other |

### CLI
- `stocks quote -` and `stocks compare -` read symbols from stdin, separated by whitespace or commas. Example: `idx stocks screen --filter high-dividend | jq -r '.[].symbol' | idx stocks compare -`.
- Financial statement line items come out in a stable order in JSON.
- Fixed: tables with colored cells (signals, change types) had a ragged right border in terminals.

### Ownership
- Fixed: the IDX workbook stores the share code `TRUE` (TRINITI DINAMIK) as a boolean cell, so its holders were imported under ticker `1`.
  - The published snapshot for 2026-09-30 has been rebuilt. The rebuild also corrects the bogus August→September changes for `TRUE` and `1`.
  - With 0.3, `idx ownership sync` picks up the rebuild automatically.
  - On 0.2.x, run `idx ownership sync --force` once.
- Sync now updates a database that is still byte-identical to an earlier build of the same snapshot. The manifest gains an optional `replaces_sqlite_sha256` field, which older clients ignore. A database you imported or edited locally still needs `--force`.

### Distribution
- Release assets add `idx-mcp-<platform>` binaries next to `idx-<platform>`, all listed in `SHA256SUMS`.
- New npm package `idx-mcp`, alongside `idx-cli`. New crates `idx-core` and `idx-mcp`.
- Rust: `idx` builds with 1.85+, and `idx-mcp` needs 1.88+.

### Verification
- 143 golden outputs (every command in table and JSON, plus error cases) are unchanged except for the documented output-default and exit-code changes.
- Live A/B checks against 0.2.4 match.
- Each MCP tool's data equals the CLI's JSON.

## [0.2.4] - 2026-09-26

### Highlights
- **Ownership data is current again.** Since June 2026 IDX publishes the monthly "Pemegang Saham di Atas 1%" list as **XLSX** on the [Data Kepemilikan Saham](https://www.idx.co.id/id/perusahaan-tercatat/data-kepemilikan-saham/) page instead of a PDF announcement. `idx ownership discover` finds it there and `idx ownership import` reads it. The published snapshot (`idx ownership sync`) now covers **2026-06-30, 2026-07-31 and 2026-08-31**.
- **New install channels:** `curl -fsSL https://raw.githubusercontent.com/0xrsydn/idx-cli/main/install.sh | sh` and npm (`npx idx-cli`, `npm i -g idx-cli`). Both use prebuilt binaries with SHA-256 verification.
- **Sync accepts larger snapshots.** v0.2.3 and earlier cap snapshot downloads at 10 MiB. This version accepts up to the manifest's size (at most 1 GiB), which leaves room for more months of history later.

### Ownership
- XLSX parser for the above-1% holder register (#25). Only the exact 12-column layout is accepted, so the daily above-5% and investor-type workbooks are rejected instead of mis-imported. Both the Feb/Mar 2026 `INVESTOR_TYPE` and the later `INVESTOR_CLASSIFICATION` headers are handled.
- Discovery merges the announcement API with the data page (#26). Downloads retry across curl-impersonate browser profiles, since IDX's Cloudflare rejects `curl_chrome142` on the new page.
- `ownership import --url` accepts `.xlsx`.
- Snapshot sync: the download is bounded by the manifest's `size_bytes`, and a body larger than advertised is rejected (#31).

### Snapshot publishing (maintainers)
- `build-latest-ownership-snapshot.sh --history <n>` includes earlier months (#27). `--max-snapshot-bytes` (default 10 MiB) refuses snapshots that released clients can't download (#31).
- Idempotent, packaged publisher: `nix build .#ownership-publisher` gives `idx-ownership-publish` and `idx-ownership-freshness`. Every run ends in one `RESULT:` line (#29). Upload verification and partial-publish recovery (#30).

### Distribution
- Release workflow builds static musl Linux (x64, arm64) and macOS (arm64, x64) binaries, plus `SHA256SUMS` and `install.sh` (#22, #24).
- npm wrapper with checksum-verified postinstall (#22). pnpm 10+ and bun need `--allow-build` / `--trust`.

### Compatibility
- The snapshot manifest is still schema v1. v0.2.3 clients keep syncing as long as snapshots stay at or below 10 MiB, which the publisher enforces.
- Discover JSON gains `format` and `as_of_date` fields; existing fields are unchanged.

## [0.2.3] - 2026-08-01

### Fixes
- handles the new spaced-header KSEI PDF layout (SHARE CODE, INVESTOR CLASSIFICATION) that broke schema classification
- normalizes spaced header text before schema classification so both underscore (SHARE_CODE) and spaced layouts parse
- adds INVESTOR_CLASSIFICATION to schema markers and header labels
- satisfies clippy 1.97 lints that failed the ubuntu CI step (filter, sort_by_key(Reverse), ?)

### Scope
- published from commit `603c0e4` (main, after PR #21 merge)
- adds a test fixture from the live June 2026 KSEI PDF and a spaced-header test case
- all 222 tests pass (143 unit + 79 integration), clippy + fmt clean
- snapshot data asset unchanged: latest KSEI above-1% report is still as_of 2026-05-29; the parser fix enables future June/July imports once IDX publishes them

## [0.2.2] - 2026-04-06

### Fixes
- handles April 2026 KSEI PDFs that omit zero-valued scrip columns on some holder rows
- defaults missing component share fields to zero while still requiring total shares
- restores successful import and sync against the live April 2026 KSEI ownership file format

### Scope
- published from commit `04eaa75`
- includes the `0.2.2` version bump that was already shipped to crates.io
- this release object backfills the missing GitHub tag/release for that published crate version

## [0.2.1] - 2026-04-06

### Highlights
- adds `idx ownership discover` to locate the latest supported IDX above-1 shareholder register PDF
- adds `idx ownership import --url <pdf-url>` for direct remote import
- adds `idx ownership sync` for maintained snapshot-based bootstrap and refresh
- adds local archive ZIP/TXT ingest as a fallback backstop path
- expands smoke coverage and fixture-backed ownership tests

### Notes
- `stocks` remains MSN-first, with Yahoo retained for history fallback
- this release is being backfilled on GitHub so release objects match the existing tag history

## [0.2.0] - 2026-03-07

### 🔍 Ownership Intelligence Module

First-of-its-kind local ownership data for all IDX-listed stocks. Import KSEI's ≥1% shareholder registry, query cross-holdings, track concentration — all offline from a single SQLite file.

#### New Commands

```bash
# Import KSEI ownership PDF (monthly release from idx.co.id)
idx ownership import --file ownership_202603.pdf
idx ownership import --url https://idx.co.id/...pdf

# Query any ticker's shareholders
idx ownership ticker BBCA
idx ownership ticker GOTO --source ksei

# Who else does this investor hold?
idx ownership entity "GOVERNMENT OF NORWAY"

# Fuzzy search entities
idx ownership search "salim"

# Cross-holding analysis
idx ownership cross-holders --top 20
idx ownership concentration --by hhi --top 10

# Ownership graph traversal
idx ownership graph BBCA --depth 2
idx ownership graph BBCA --format dot  # Graphviz export

# Diff between monthly releases
idx ownership changes --from 2026-01-27 --to 2026-02-27

# Entity resolution tools
idx ownership resolve unresolved
idx ownership resolve map "PT ASTRA INTL" "ASTRA INTERNATIONAL"
idx ownership resolve merge 42 99
```

#### Highlights
- **KSEI PDF parser** — mutool stext + quick-xml, 99.2% accuracy across 73-page PDFs
- **7,256 ownership rows** across **955 tickers** from a single import
- **FTS5 trigram search** on 5,175 entities
- **Graph traversal** with recursive CTEs (ASCII tree + Graphviz DOT)
- **Entity resolution CLI** — manual mapping + merge for deduplication
- **SQLite-backed** — single `ownership.db` file, zero infrastructure
- **Feature-gated** — `--features ownership` (default-on)

#### Also in this release
- Hybrid history provider: MSN default + Yahoo fallback (`--history-provider auto|yahoo|msn`)
- Bing Finance institutional ownership API client (5 endpoints, ready for integration)
- `docs/ARCHITECTURE.md` and `docs/CONVENTIONS.md`

#### Stats
- 82 tests passing (unit + integration)
- QA validated: 41/41 pass across 20 sample tickers

## [0.1.0] - 2026-03-05

The first tagged version: quotes, history, fundamentals and screens from MSN and Yahoo. It was published without release notes.
