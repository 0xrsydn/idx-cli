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
- [x] 142 tests passing (94 unit + 48 integration)

## ✅ Completed (2026-03-26 — hardening pass)
- [x] Unified cache, offline, stale-cache, and `--no-cache` behavior across core and MSN-only stock commands
- [x] Rejected the conflicting `--offline --no-cache` flag combination explicitly
- [x] Routed startup/config failures through the same JSON-aware error path as runtime failures
- [x] Validated `stocks screen --filter` and `--region`; invalid values now return errors
- [x] Added regression tests for startup JSON errors, MSN profile offline/stale-cache behavior, and screener validation

## ✅ Completed (2026-03-26 — MSN output cleanup)
- [x] Re-mapped live MSN `profile` output to prefer localized/company fields for name, description, sector, industry, website, address, and phone
- [x] Reworked `stocks insights` output to derive a real summary, split highlights vs risks from evaluation status, and populate `last_updated`
- [x] Fixed signed-number formatting so `stocks financials` table output no longer mangles negative values
- [x] Added live-like fixtures and regression coverage for `profile`, `insights`, and signed table formatting

## 🚧 Next Up
- [x] Build a reusable live smoke script/checklist for all shipped CLI commands (`scripts/live-smoke.sh`, `docs/SMOKE.md`)
- [x] Re-run full live smoke for MSN-only commands after the cache/offline, mapping, and formatting fixes (`scripts/live-smoke.sh --mode full --group live-table --group live-json`)
- [x] Review remaining noisy table output in `financials` and `earnings` beyond the signed-number fix
- [x] Expand parser/CLI regression coverage for the rest of the MSN-only command set
- [x] Keep the live-smoke notes below in sync with real command output after each hardening pass

## 🚧 Ownership Roadmap Reset (2026-03-29)

### Batch 1 — IDX discovery + remote PDF import
- [x] Verify and document the IDX announcement/listing page that exposes the monthly ownership PDF link
  - Verified official BEI listing page: `https://www.idx.co.id/id/berita/pengumuman/`
  - Verified listing page JSON source used by the site: `GET /primary/NewsAnnouncement/GetAllAnnouncement?keywords=...`
- [x] Implement discovery/crawler logic for the hashed IDX PDF asset URL instead of hardcoding monthly paths
- [x] Expose discovery as an explicit `idx ownership discover` CLI surface so the live BEI feed can be inspected without coupling it to import
- [x] Extract a reusable browser-impersonated fetch helper for ownership downloads by reusing the repo's `curl-impersonate` pattern
- [x] Wire `ownership import --url` to the impersonated fetch path for IDX-hosted PDFs
- [x] Keep the current PDF parser/import path as the first production ingest route
- [x] Add tests and fixtures for IDX announcement-page parsing plus remote PDF fetch failure modes
- [x] Batch 1 verification: `cargo build`
- [x] Batch 1 verification: `cargo clippy -- -D warnings`
- [x] Batch 1 verification: `cargo test`
- [x] Batch 1 verification: fixture-backed parser/downloader tests cover announcement discovery, hashed URL extraction, and downloader failure cases
- [x] Batch 1 verification: live `idx ownership discover --family above1 --limit 2` in `nix develop` resolves the current BEI hashed URLs for the parser-compatible `above 1%` report family
- [x] Batch 1 verification: live end-to-end import succeeds for one discovered BEI ownership PDF URL
- [x] Batch 1 verification: end-to-end import into a temp ownership DB via `idx ownership import --url ...`, followed by `idx ownership releases` and one ticker query
  - Resolved root cause on `2026-03-29`:
    - the parser was already correct for the holder-register schema; the missing piece was discovery support for the `Pemegang Saham di atas 1% (KSEI)` family
    - the parser-compatible source is the `lamp1` attachment `https://www.idx.co.id/StaticData/NewsAndAnnouncement/ANNOUNCEMENTSTOCK/From_EREP/202603/b9b638e5a8_8928aca255.pdf`
    - the `above 5%` and `investor-type` BEI families remain different schemas; as of `2026-03-30`, they should be treated as legacy / unsupported input rather than new parser targets
  - Post-merge reconfirmation on `2026-03-30`:
    - live `idx ownership discover --family above1 --limit 2` still resolves the `2026-03-10` `above 1%` BEI pair and keeps the `lamp1` attachment as the parser-compatible source
    - live `idx ownership import --url` into an isolated temp DB succeeded again from `b9b638e5a8_8928aca255.pdf`: `7261` rows for `955` tickers, `1` release, `5200` distinct raw investor names, and `5176` canonical entities for `as_of_date=2026-02-27`
    - post-import sanity checks still pass for `idx ownership releases` and `idx ownership ticker AADI --source ksei`
    - parser coverage now includes a compact real `above1` `mutool 1.27.0` `stext` excerpt fixture, not only the synthetic live-like line fixture

### Batch 2 — Above-1 hardening and unsupported-input UX
- [x] Standardize the supported remote-import contract on the `Pemegang Saham di atas 1% (KSEI)` holder-register layout and its `lamp1` attachment shape
- [x] Add BEI PDF schema classification before parse/import so `ownership import --url` can reject non-holder-register PDFs before the parser runs
- [x] Improve CLI error messages and fallback behavior for discovery failure, fetch failure, invalid remote content, and known-but-unsupported legacy BEI schema variants
- [x] Capture live-like fixtures for the current discoverable `investor-type` and `above 5%` BEI attachments (or their `mutool` `stext` extracts) so unsupported-input detection and failure UX are regression-tested
- [x] Decide and document whether `ownership discover` should default to `above1` output while keeping legacy families available only for diagnostic use
- [x] Decide and document whether `ownership import --url` accepts only direct PDF URLs or can also accept an IDX listing page as input
- [x] Add regression coverage for Cloudflare/HTML responses, missing announcement links, duplicate release imports, and unsupported BEI schema detections
- [x] Batch 2 verification: `cargo build`
- [x] Batch 2 verification: `cargo clippy -- -D warnings`
- [x] Batch 2 verification: `cargo test`
- [x] Batch 2 verification: ownership-focused smoke checks cover successful remote import plus expected failure UX

### Batch 3 — Snapshot publishing + sync
- [x] Design maintained SQLite snapshot publishing after remote IDX import is stable
- [x] Add `idx ownership sync`
- [x] Define manifest/checksum/update semantics and local DB replacement rules
- [x] Add regression coverage for manifest parsing, checksum validation, no-op sync, and forced refresh
- [x] Batch 3 verification: `cargo build`
- [x] Batch 3 verification: `cargo clippy -- -D warnings`
- [x] Batch 3 verification: `cargo test`
- [x] Batch 3 verification: sync installs into an empty temp data dir, preserves query behavior, and no-ops when already current

### Batch 4 — KSEI ZIP/TXT fallback and cross-check path
- [x] Keep KSEI ZIP/TXT ingest as fallback and validation/backstop work, not the first milestone
- [x] Define whether the KSEI archive is only a maintainer fallback or a user-facing alternative import source
- [x] Add cross-check coverage between IDX-PDF-derived output and KSEI-archive-derived output for at least one monthly release
- [x] Batch 4 verification: `cargo build`
- [x] Batch 4 verification: `cargo clippy -- -D warnings`
- [x] Batch 4 verification: `cargo test`
- [x] Batch 4 verification: fallback ingest produces a compatible SQLite state for `ownership releases`, `ticker`, and `changes`

## 🎯 Current Core Work (from FEATURE_SPEC.md)

### P0 — Correctness and architecture
- [x] Unify provider and capability flow so `src/cli/stocks.rs` stops constructing `MsnProvider` directly for MSN-only command handlers
- [x] Fix screener row hygiene so incomplete MSN screener rows with missing price data are filtered or rejected instead of defaulting to `0.0`
- [x] Decide and implement the fundamentals fallback policy when company metrics are missing
- [x] Harden Yahoo reliability edge cases: intermittent `429` handling and documented/intentional SMA200 behavior with fewer than `200` candles

### P1 — UX and output contract cleanup
- [x] Add `stocks financials` filters such as `--statement income|balance|cashflow`
- [x] Add `stocks earnings` filters such as `--forecast|--history` and `--annual|--quarterly`
- [x] Review JSON payload consistency where symbol or context fields are still sparse
- [x] Decide whether `screen` stays under `stocks` long term or graduates into a richer dedicated surface later

### P2 — Deferred but real work
- [ ] Add MSN chart/history support through `stocks history --history-provider msn`
- [ ] Define and implement `ownership import --fetch-bing`
- [ ] Decide whether richer financial statements should stay single-period or grow into multi-period fetch support

## 🚀 Publish Readiness (2026-04-03)

### P1 — Release blockers
- [x] Keep the installed binary name as `idx`; `Cargo.toml` already ships `[[bin]] name = "idx"` while the package remains `idx-cli`
- [x] Expose a default Nix package/app so the documented `nix run github:0xrsydn/idx-cli` path actually works
- [x] Restrict packaged crate contents so internal repo files like `.claude/`, `CLAUDE.md`, `AGENTS.md`, and agent-planning docs are not shipped to crates.io
- [x] Document real runtime dependencies and platform assumptions for `cargo install idx-cli`, including `curl-impersonate-chrome` for Yahoo auth and `mutool` for ownership PDF parsing
- [x] Add release/install verification to CI, at minimum `cargo package` and an install smoke path; include Nix app/package verification if the README keeps advertising `nix run`

### P2 — Pre-publish cleanup
- [x] Wire `--quiet` so non-essential `info:` / `warning:` output is actually suppressed
- [x] Fail fast on invalid `IDX_CACHE_QUOTE_TTL` / `IDX_CACHE_FUNDAMENTAL_TTL` env values instead of silently ignoring them
- [x] Propagate `-v` into the history/technical provider path so verbose Yahoo diagnostics can surface
- [x] Honor XDG overrides consistently for ownership DB/raw download defaults, not only config/cache
- [x] Tighten `scripts/live-smoke.sh` so it does not silently validate a stale `target/debug/idx` binary
- [x] Remove or fix the README reference to the non-existent `skills/` directory

## 📋 Backlog (per SPEC.md)
- [ ] `market summary` — IHSG index, market breadth
- [ ] `market movers` — top gainers/losers/volume
- [ ] `market sectors` — sector performance overview
- [ ] `screen query "<EXPR>"` — filter stocks by expression
- [ ] `screen presets` / `screen run <PRESET>` — built-in screener presets
- [ ] `watchlist` commands — create, manage, live watch
- [ ] `alerts` system (v0.2+) — price alerts with daemon
- [x] `completions <SHELL>` — shell completion generation
- [ ] CSV/TSV output formats
- [ ] Additional providers (Alpha Vantage, Twelve Data, IDX official)

## 🔬 Latest Smoke Findings (2026-04-02)
- [x] Final release-hygiene pass on `2026-04-06`: crate metadata now declares `rust-version = 1.85`, README install docs now spell out Cargo helper-runtime expectations plus persistent `nix profile install`, and CI install smoke now runs the mock smoke matrix against the installed binary instead of only checking `idx version`
- [x] Verification on `2026-04-06`: `nix develop --command cargo build`, `nix develop --command cargo clippy -- -D warnings`, `nix develop --command cargo test`, `nix develop --command cargo package --allow-dirty --locked`, and `scripts/live-smoke.sh --bin ./tmp/release-install/bin/idx --no-build --mode mock` all passed
- [x] Publish review blocker batch on `2026-04-05`: the default Nix app/package path builds again without relying on an untracked runtime module, `ownership import --force` now re-imports the same release SHA atomically, and current ownership views (`ticker`, `entity`, `cross-holders`, `concentration`, `graph`) now scope KSEI data to the latest imported release instead of blending historical snapshots
- [x] Verification on `2026-04-05`: `nix develop --command cargo build`, `nix develop --command cargo clippy -- -D warnings`, `nix develop --command cargo test`, `nix build .#default`, `nix develop --command cargo package --allow-dirty --locked`, fresh `cargo install --path . --locked --root tmp/release-install`, `./tmp/release-install/bin/idx version`, and `nix run .#default -- version` all passed
- [x] Publish P2 cleanup batch on `2026-04-03`: `--quiet` now suppresses non-essential CLI warnings/info, invalid cache TTL env vars fail during startup, `-v` reaches Yahoo history diagnostics, ownership default DB/raw paths honor XDG overrides, and `scripts/live-smoke.sh` now refuses a stale `target/debug/idx` when build refresh is skipped
- [x] Verification on `2026-04-03`: `nix develop --command cargo build`, `nix develop --command cargo clippy -- -D warnings`, `nix develop --command cargo test`, `bash -n scripts/live-smoke.sh`, and `scripts/live-smoke.sh --mode mock --dry-run` all passed
- [x] Publish blocker batch on `2026-04-03`: crate packaging now excludes repo-internal agent harness files, the flake exports a default package/app for `nix run`, README install docs now describe runtime helper dependencies, and CI now checks package/install surfaces
- [x] Verification on `2026-04-03`: `nix develop --command cargo build`, `nix develop --command cargo clippy -- -D warnings`, `nix develop --command cargo test`, `nix develop --command cargo package --allow-dirty --locked`, fresh `cargo install --path . --locked --root tmp/release-install`, `./tmp/release-install/bin/idx version`, `nix build .#default`, and `nix run .#default -- version` all passed
- [x] P0 provider/capability routing is now centralized through `SelectedProvider`; `stocks` handlers no longer construct `MsnProvider` directly for MSN-only commands
- [x] MSN fundamentals now reject industry-only metrics with an explicit unsupported error; the normal MSN mock/live path uses company metrics only
- [x] MSN screener parsing now drops rows with missing/invalid/non-positive price data instead of synthesizing `0.0` price rows
- [x] Yahoo `429` retry/backoff is now centralized and regression-tested for chart and quote-summary fetches
- [x] `stocks technical` table output now says `Trend unavailable (need at least 200 daily candles)` when fewer than `200` daily candles are available
- [x] Verification on `2026-04-02`: `nix develop --command cargo build`, `nix develop --command cargo clippy -- -D warnings`, `nix develop --command cargo test`, `scripts/live-smoke.sh --mode mock`, and `scripts/live-smoke.sh --group live-table --group live-json --group routing --group cache --group errors` all passed (`tmp/live-smoke/20260402-122215`, `tmp/live-smoke/20260402-122218`)
- [x] Live smoke passed for shipped `stocks` commands: `quote`, `history`, `technical`, `growth`, `valuation`, `risk`, `fundamental`, `compare`, `profile`, `financials`, `earnings`, `sentiment`, `insights`, `news`, `screen`
- [x] Yahoo routing verified for live `quote` and `history`
- [x] `stocks history --history-provider msn` correctly fails for IDX as unsupported
- [x] `ownership releases` works with a writable `ownership.db_path` and an empty DB
- [x] Regression coverage now verifies offline/cache parity for MSN-only commands (`stocks profile BBCA`)
- [x] `--offline --no-cache` now fails fast as an invalid flag combination instead of serving cache
- [x] Config/startup failures now honor the JSON error contract (`IDX_PROVIDER=bogus idx -o json version`)
- [x] Invalid `stocks screen --filter/--region` values now return validation errors
- [x] Added reusable smoke runner/checklist for shipped CLI surfaces (`scripts/live-smoke.sh`, `docs/SMOKE.md`)
- [x] Reusable smoke runner passes on deterministic mock suites: `general`/`cache`/`routing`/`errors`/`ownership` = 32/32 and shipped `stocks` mock matrix = 30/30 (`tmp/live-smoke/20260326-194907`, `tmp/live-smoke/20260326-194914`)
- [x] Full live MSN-only smoke now passes via runner for both table and JSON surfaces: 30/30 (`tmp/live-smoke/20260326-195853`)
- [x] Targeted no-cache live checks confirm `profile`, `insights`, and `financials` fixes against real MSN responses
- [x] Live `stocks profile BBCA` no-cache output now populates company/localized fields instead of the sparse top-level fallback
- [x] Live `stocks insights BBCA` JSON now returns a mixed-signal summary plus non-empty `last_updated`
- [x] Live `stocks financials BBCA` table no longer renders malformed negative numbers
- [x] Re-run full live smoke to reconfirm all MSN-only commands after the latest hardening fixes
- [x] `stocks financials BBCA` table now trims ISO timestamps from section headers and humanizes raw line-item keys
- [x] `stocks earnings BBCA` table now splits history vs forecast and formats annual periods, revenue values, and dates for table mode
- [x] `stocks financials` now supports section filters via `--statement income|balance|cashflow`, with filtered table output and JSON sections rendered as `null` when intentionally excluded
- [x] `stocks earnings` now supports `--forecast|--history` and `--annual|--quarterly`, so table and JSON output can be scoped without changing the cached source payload
- [x] JSON payload context is now less sparse for MSN `earnings`, `insights`, and `news`; each payload now carries the resolved stock symbol even when sourced from older cache entries
- [x] `stocks financials` JSON now normalizes `instrument.symbol` to the exchange-qualified ticker (for example `BBCA.JK`), and older cached payloads are backfilled at read time so users do not need to clear cache after upgrading
- [x] Product decision on `2026-04-02`: keep `screen` under `stocks` for now; revisit a dedicated surface only when `screen query` / `screen presets` graduate from backlog into a richer workflow
- [x] Fixture-backed parser and CLI JSON regression coverage now covers the remaining MSN-only `sentiment`, `news`, and `screen` commands
- [x] Fresh post-coverage live MSN smoke rerun still passes for table and JSON surfaces: 30/30 (`tmp/live-smoke/20260327-163403`)
- [ ] `ownership import --fetch-bing` is still deferred and returns unsupported
- [x] Real KSEI PDF import from local file now works again: `ownership import --file /Users/rasyidanakbar/Downloads/ksei_raw_data.pdf` imported `7261` rows for `955` tickers on `2026-03-28`, replacing the previous `6`-row/`1`-ticker failure mode
- [x] KSEI parser no longer depends on the old hardcoded column bounds fixture layout; it now reconstructs rows from `mutool` line output and handles the live `DATE + SHARE_CODE` merged segment plus `D`/`A` locality markers
- [x] A real IDX-hosted March 2026 ownership PDF URL was verified on `2026-03-29`, but only through `curl-impersonate` inside `nix develop`; plain `curl` still returns `403` from Cloudflare for the same asset
- [x] The current repo now has a concrete IDX-first roadmap: discover the hashed PDF URL from IDX pages, fetch it with browser impersonation, and only then layer snapshot publishing and `ownership sync`
- [x] The KSEI archive (`https://web.ksei.co.id/archive_download/holding_composition`) was verified as a secondary upstream that exposes monthly ZIP files and remains useful for fallback/cross-check work
- [x] Remote IDX discovery now targets the official BEI `Pengumuman` feed (`/id/berita/pengumuman/`) and its backing JSON endpoint (`/primary/NewsAnnouncement/GetAllAnnouncement`), which exposes hashed attachment URLs for ownership-related reports
- [x] New `ownership discover` CLI output now shows the latest hashed BEI ownership URLs across the current `above 5%`, `above 1%`, and `investor-type` report families
- [x] `ownership import --url https://www.idx.co.id/...pdf` now uses the same browser-impersonated download path as Yahoo auth
- [x] Reverse-engineering the current BEI feed shows the discoverable `Pemegang Saham di atas 1% (KSEI)` `lamp1` attachment matches the known-good raw KSEI holder-register layout and now imports successfully
- [x] Live `ownership import --url` against the discovered `above 1%` BEI `lamp1` attachment (`b9b638e5a8_8928aca255.pdf`) now succeeds end to end: `7261` rows for `955` tickers on `2026-03-29`, followed by successful `ownership releases` and `ownership ticker AADI --source ksei`
- [x] Product scope decision on `2026-03-30`: standardize supported remote import on the discoverable `above 1%` holder-register family; treat `above 5%` and `investor-type` PDFs as legacy / unsupported input
- [x] Live `ownership import --url` checks against the currently discoverable `above 5%` and `investor-type` BEI PDFs still fail with `no KSEI rows parsed from PDF`, which is expected until unsupported-family detection / rejection UX lands
- [x] Live `mutool` inspection of the current discoverable `investor-type` BEI attachment shows a stock-level aggregate matrix (`DATE`, `STOCK_CODE`, `NUMBER_OF_SHARES`, investor-type columns, holder-size buckets), not the holder-level KSEI register schema the current parser imports
- [x] Live `mutool` inspection of the current discoverable `above 5%` BEI attachment shows a member/tampungan report (`INVS`, member names, `KSEI UNTUK CLOSED MEMBER-...` labels), not the raw KSEI holder-register layout
- [x] `ownership discover` now defaults to the supported `above1` family, surfaces per-URL importability status, and orders the importable `lamp1` attachment first so `--limit 1` yields the current supported PDF URL
- [x] `ownership import --url` now accepts only direct PDF URLs; IDX listing/announcement page URLs fail fast with guidance to run `ownership discover`
- [x] Valid-but-unsupported BEI PDFs now fail before row parsing with explicit schema-aware errors (`announcement_wrapper`, legacy `above5`, legacy `investor-type`) instead of the old generic `no KSEI rows parsed from PDF` path
- [x] Regression coverage now covers default `ownership discover` behavior, status visibility, listing-page rejection, duplicate SHA imports, and explicit unsupported-schema detection with compact `stext` fixtures plus fake-`mutool` CLI tests
- [x] New `ownership-import` smoke coverage now discovers the current live `above1`/`above5`/`investor-type` URLs, imports the supported `above1` attachment successfully, and confirms the legacy families fail with explicit unsupported-schema UX (`tmp/live-smoke/20260330-160201`)
- [x] New ownership snapshot sync coverage now verifies manifest parsing, checksum validation, install into an empty temp data dir, preserved query behavior for `releases`/`ticker`/`changes`, no-op sync when current, and `--force` refresh via fixture-backed local manifests on `2026-03-31`
- [x] KSEI archive ZIP/TXT fallback import now works through `ownership import --file` for local `.zip` and `.txt` inputs, using synthetic investor-type/locality aggregate holders as a maintainer backstop rather than the primary product ingest
- [x] Batch 4 coverage now cross-checks the live-like `2026-02-27` `above1` PDF fixture against the matching KSEI archive bucket excerpt and verifies fallback import/query behavior for `releases`, `ticker`, and `changes`

## 🐛 Known Issues
- [ ] Yahoo Finance returns 429 from datacenter IPs occasionally
- [ ] SMA200 trend shows "Insufficient data" if Yahoo returns < 200 candles
- [x] KSEI ownership parser no longer leaks adjacent columns into `INVESTOR_NAME` on the March 2026 live PDF; direct CLI verification now returns clean holders like `AGUNG PERKASA INVESTINDO` / `CP` / `L`
- [x] Yahoo quoteSummary crumb auth — fixed via curl-impersonate-chrome (curl_chrome131)
  - fc.yahoo.com → A3 cookie + query1 getcrumb → crumb, both sent to quoteSummary
  - Requires nixpkgs#curl-impersonate-chrome in PATH (added to flake.nix + clan-private)
