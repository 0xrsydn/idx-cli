# Feature Spec: Current Coverage and Remaining Core Work

**Status:** Active - updated against the current repo state on 2026-03-31
**Current focus:** Close the remaining core gaps before expanding into new market and watchlist surfaces
**Reference:** `origin/dev/rubick` (endpoint reference only, not the current implementation plan)

---

## Purpose

This is no longer a "port MSN from zero" spec.

The CLI already ships MSN-backed support for:
- `profile`
- `financials`
- `earnings`
- `sentiment`
- `insights`
- `news`
- `screen`

Use this document to track what is still open in core functionality.
Use `TODO.md` as the execution log and smoke-history record.

---

## Verified Current State

- Current automated coverage is `180` tests: `117` unit and `63` integration.
- Reusable smoke coverage exists via `scripts/live-smoke.sh`; command groups are documented in `docs/SMOKE.md`.
- The latest smoke notes in `TODO.md` report passing live table and JSON checks for all shipped `stocks` commands.
- Cache/offline parity, JSON startup-error handling, screener input validation, and the recent MSN output cleanups have already been completed.
- KSEI ownership import/query is now verified end-to-end from a local March 2026 PDF file into SQLite, with direct CLI reads through `ownership releases` and `ownership ticker`.
- Remote ownership ingestion should now be treated as an IDX PDF discovery-and-fetch problem first, not a hardcoded KSEI PDF URL problem: direct IDX announcement PDFs exist, but the hashed asset URL must be discovered from an IDX listing/announcement surface and fetched with browser-like behavior.
- The CLI now has an explicit `idx ownership discover` surface that enumerates the latest hashed BEI ownership report URLs and defaults to the supported `above1` family.
- Live verification on `2026-03-29` confirmed that the parser-compatible discovered source is currently the `Pemegang Saham di atas 1% (KSEI)` `lamp1` attachment, and `idx ownership import --url` now works end to end for that discovered BEI PDF.
- The currently discoverable `above 5%` and `investor-type` BEI families remain different schemas and are now classified and rejected explicitly during import instead of falling through to a generic zero-row parse failure.
- Ownership smoke coverage now includes a dedicated `ownership-import` group that verifies live supported import plus expected unsupported-family failures.
- `idx ownership sync` now installs maintained SQLite snapshots via a manifest-driven contract with checksum validation, conservative local replacement rules, and fixture-backed regression coverage for install/no-op/force-refresh behavior.
- The KSEI archive ZIP/TXT path is now implemented as a local-file maintainer fallback/backstop, with cross-check coverage against the `above1` PDF contract for a shared monthly release.

This means the main gap is no longer endpoint coverage.
The remaining work is architecture cleanup, a few correctness edge cases, and selective UX expansion on top of already-shipped commands.

---

## Coverage Snapshot

| Area | CLI | Status | Notes |
| --- | --- | --- | --- |
| Quotes | `idx stocks quote` | Implemented | Cached, smoke-tested, and covered by integration tests |
| History | `idx stocks history` | Partial | Works today via Yahoo/history-provider routing; explicit MSN history remains unsupported for IDX |
| Technical | `idx stocks technical` | Implemented | Uses the cached history path |
| Growth | `idx stocks growth` | Implemented | Shipped and exercised |
| Valuation | `idx stocks valuation` | Implemented | Shipped and exercised |
| Risk | `idx stocks risk` | Implemented | Shipped and exercised |
| Fundamental | `idx stocks fundamental` | Implemented | Shipped and exercised |
| Compare | `idx stocks compare` | Implemented | Shipped and exercised |
| Company profile | `idx stocks profile` | Implemented | Cache/offline parity and fixture-backed coverage are in place |
| Financial statements | `idx stocks financials` | Implemented | Output cleanup landed, and statement filters now support `--statement income|balance|cashflow` |
| Earnings | `idx stocks earnings` | Implemented | History/forecast split is rendered, and filters now support `--forecast|--history` with `--annual|--quarterly` |
| Sentiment | `idx stocks sentiment` | Implemented | Fixture-backed CLI coverage exists |
| Insights | `idx stocks insights` | Implemented | Summary/highlights/risks/`last_updated` mapping was corrected and tested |
| News | `idx stocks news` | Implemented | Fixture-backed CLI coverage exists |
| Screener | `idx stocks screen` | Implemented with gaps | Validation landed; expression/preset workflow is still future work |
| MSN charts | `idx stocks history --history-provider msn` | Missing | Explicit MSN history still returns unsupported for IDX |
| KSEI ownership import/query | `idx ownership import --file`, `idx ownership import --url`, `idx ownership releases`, `idx ownership ticker` | Implemented | Local PDF import and SQLite-backed query flow are verified against the March 2026 KSEI release; remote IDX import now works for the discovered `above 1%` `lamp1` BEI attachment, and legacy `above 5%` / `investor-type` BEI report families are rejected explicitly |
| KSEI archive fallback import | `idx ownership import --file <.zip|.txt>` | Implemented as fallback | Local archive ZIP/TXT ingest maps investor-type/locality buckets into synthetic aggregate holders for validation/backstop use, not the primary ingest surface |
| Ownership snapshot sync | `idx ownership sync` | Implemented | Manifest-driven SQLite snapshot install with checksum validation, conservative replacement/no-op rules, and publisher helper script |
| Bing ownership CLI | `idx ownership import --fetch-bing` | Not implemented | Client groundwork exists, CLI import path is still deferred |

---

## Resolved Since The Previous Revision

The following items should no longer be treated as active backlog in this spec:

- CLI truth-pass baseline for shipped `stocks` commands
- Unified cache, offline, stale-cache, and `--no-cache` handling across the core and MSN-only stock commands
- Rejection of the conflicting `--offline --no-cache` flag combination
- JSON-aware startup/config failures, not just runtime failures
- Validation for `stocks screen --filter` and `--region`
- `profile` output remapping to prefer company/localized fields
- `insights` output remapping for summary, highlights, risks, and `last_updated`
- Signed-number and table-label cleanup for `financials`
- Table-mode cleanup for `earnings`
- Baseline parser and CLI regression coverage for the shipped MSN-only command set
- KSEI ownership parser hardening for the March 2026 live PDF layout, including the merged `DATE + SHARE_CODE` segment and `D`/`A` locality markers
- Real KSEI ownership CLI verification from local file import into SQLite (`7261` rows across `955` tickers on `2026-03-28`)
- Ownership remote-import hardening for the `above1` contract, including direct-PDF-only `--url` input, discovery status output, explicit legacy-schema rejection, and live ownership-import smoke coverage
- Ownership snapshot publishing/sync contract, including `idx ownership sync`, manifest/checksum validation, conservative local replacement rules, and the `scripts/build-ownership-snapshot.sh` publisher helper
- KSEI archive ZIP/TXT fallback ingest for local `.zip`/`.txt` files, including cross-check coverage against the `above1` PDF holder-register fixture and compatible `releases`/`ticker`/`changes` verification

If any of the above regress, capture that in `TODO.md` as a new finding rather than reopening the old section here wholesale.

---

## Remaining Core Gaps

### P0 - Correctness and architecture

#### 1. Unify provider and capability flow

Current state:
- `src/cli/stocks.rs` still directly constructs `MsnProvider::new(false)` for `profile`, `financials`, `earnings`, `sentiment`, `insights`, `news`, and `screen`.
- This keeps a split execution path alive even though cache/offline behavior is now mostly unified around `fetch_msn_with_cache`.

Why it matters:
- The architecture doc describes provider/capability-based flow, but the CLI still special-cases MSN-only commands at the handler layer.
- Future features will be harder to extend cleanly if this split remains.

Done when:
- Normal command handlers stop constructing `MsnProvider` directly.
- Provider selection and capability checks are centralized and consistent with `docs/ARCHITECTURE.md`.

#### 2. Fix screener row hygiene for incomplete data

Current state:
- `src/api/msn/map.rs` still defaults missing screener price data to `0.0` when constructing `Quote` rows.

Why it matters:
- A zero-priced row is not the same thing as a valid priced stock.
- This can silently admit incomplete market rows instead of rejecting or filtering them.

Done when:
- Screener rows without usable price data are filtered or rejected explicitly.
- Regression tests cover the chosen behavior.

#### 3. Decide the fundamentals fallback policy

Current state:
- Fundamentals can still fall back to industry-level metrics when company metrics are absent.

Why it matters:
- This can produce analysis that looks precise but is actually based on peer or category data.

Decision needed:
- Allow the fallback and annotate it clearly, or
- reject the fallback and surface missing company data explicitly.

Done when:
- The policy is explicit in code and reflected in output semantics.

#### 4. Harden Yahoo reliability edge cases

Open issues:
- Yahoo can still return `429` from datacenter IPs intermittently.
- SMA200 trend output can still show "Insufficient data" when fewer than `200` candles are returned.

Done when:
- The retry/fallback story for Yahoo failures is deliberate and documented.
- SMA200 behavior is either improved or clearly documented as expected.

### P1 - UX and output contract cleanup

Completed on `2026-04-02`:
- `financials` now supports `--statement income|balance|cashflow`.
- `earnings` now supports `--forecast|--history` and `--annual|--quarterly`.
- JSON payload context is less sparse for MSN `earnings`, `insights`, and `news`; each now carries the resolved symbol.
- `screen` stays under `stocks` for now; revisit a dedicated surface only if `screen query` / `screen presets` grow into a richer workflow.

Done when:
- Existing shipped commands are easier to drive without changing product scope.

### P2 - Next feature work after the above is green

Priority order:

1. MSN Charts / `Finance/Charts`
   - Reuse the existing `idx stocks history` command.
   - Decide how to handle price-only timeframes safely.

2. Bing ownership CLI integration
   - Reuse the existing client groundwork in `src/api/msn/bing.rs`.
   - Define the import shape and output contract for `idx ownership import --fetch-bing`.

3. Richer financial statements
   - Decide whether to stay with the current single-period model or add multi-period fetch support.

4. New user-facing surfaces from `TODO.md`
   - `market summary`
   - `market movers`
   - `market sectors`
   - `screen query`
   - `screen presets`
   - `watchlist`
   - `alerts`

---

## Verification Gate

Do not treat a core refactor or new provider feature as complete until all of the following are true:

- `cargo build` passes
- `cargo clippy -- -D warnings` passes
- `cargo test` passes
- `scripts/live-smoke.sh --mode mock` passes
- the relevant live smoke groups pass for changed user-facing behavior
- `TODO.md` is updated with any new smoke finding, regression, or behavior change

Keep the detailed reusable smoke commands in `docs/SMOKE.md`.
Do not duplicate per-run results in this spec.

---

## Endpoint Reference Appendix

MSN API key (public, embedded in MSN Money website):

```
0QfOX3Vn51YCzitbLaRkTTBadtWpgTN8NZLW0C1SEM
```

Base URLs:
- `https://assets.msn.com/service/` - core market data (Quotes, Charts, Equities, Earnings, Sentiment, Screener)
- `https://api.msn.com/msn/v0/pages/finance/` - extended data (key ratios, insights, news feed)
- `https://services.bingapis.com/contentservices-finance.hedgefunddataprovider/api/v1/` - Bing ownership data

Keep this appendix for endpoint discovery and future work.
Use the sections above as the actual implementation plan.
