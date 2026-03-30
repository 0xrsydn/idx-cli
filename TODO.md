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

### Batch 2 — Above-1 hardening and unsupported-input UX
- [ ] Standardize the supported remote-import contract on the `Pemegang Saham di atas 1% (KSEI)` holder-register layout and its `lamp1` attachment shape
- [ ] Add BEI PDF schema classification before parse/import so `ownership import --url` can reject non-holder-register PDFs before the parser runs
- [ ] Improve CLI error messages and fallback behavior for discovery failure, fetch failure, invalid remote content, and known-but-unsupported legacy BEI schema variants
- [ ] Capture live-like fixtures for the current discoverable `investor-type` and `above 5%` BEI attachments (or their `mutool` `stext` extracts) so unsupported-input detection and failure UX are regression-tested
- [ ] Decide and document whether `ownership discover` should default to `above1` output while keeping legacy families available only for diagnostic use
- [ ] Decide and document whether `ownership import --url` accepts only direct PDF URLs or can also accept an IDX listing page as input
- [ ] Add regression coverage for Cloudflare/HTML responses, missing announcement links, duplicate release imports, and unsupported BEI schema detections
- [ ] Batch 2 verification: `cargo build`
- [ ] Batch 2 verification: `cargo clippy -- -D warnings`
- [ ] Batch 2 verification: `cargo test`
- [ ] Batch 2 verification: ownership-focused smoke checks cover successful remote import plus expected failure UX

### Batch 3 — Snapshot publishing + sync
- [ ] Design maintained SQLite snapshot publishing after remote IDX import is stable
- [ ] Add `idx ownership sync`
- [ ] Define manifest/checksum/update semantics and local DB replacement rules
- [ ] Add regression coverage for manifest parsing, checksum validation, no-op sync, and forced refresh
- [ ] Batch 3 verification: `cargo build`
- [ ] Batch 3 verification: `cargo clippy -- -D warnings`
- [ ] Batch 3 verification: `cargo test`
- [ ] Batch 3 verification: sync installs into an empty temp data dir, preserves query behavior, and no-ops when already current

### Batch 4 — KSEI ZIP/TXT fallback and cross-check path
- [ ] Keep KSEI ZIP/TXT ingest as fallback and validation/backstop work, not the first milestone
- [ ] Define whether the KSEI archive is only a maintainer fallback or a user-facing alternative import source
- [ ] Add cross-check coverage between IDX-PDF-derived output and KSEI-archive-derived output for at least one monthly release
- [ ] Batch 4 verification: `cargo build`
- [ ] Batch 4 verification: `cargo clippy -- -D warnings`
- [ ] Batch 4 verification: `cargo test`
- [ ] Batch 4 verification: fallback ingest produces a compatible SQLite state for `ownership releases`, `ticker`, and `changes`

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

## 🔬 Latest Smoke Findings (2026-03-28)
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

## 🐛 Known Issues
- [ ] Yahoo Finance returns 429 from datacenter IPs occasionally
- [ ] SMA200 trend shows "Insufficient data" if Yahoo returns < 200 candles
- [x] KSEI ownership parser no longer leaks adjacent columns into `INVESTOR_NAME` on the March 2026 live PDF; direct CLI verification now returns clean holders like `AGUNG PERKASA INVESTINDO` / `CP` / `L`
- [x] Yahoo quoteSummary crumb auth — fixed via curl-impersonate-chrome (curl_chrome131)
  - fc.yahoo.com → A3 cookie + query1 getcrumb → crumb, both sent to quoteSummary
  - Requires nixpkgs#curl-impersonate-chrome in PATH (added to flake.nix + clan-private)
