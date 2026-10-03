# Smoke Checks

Use `scripts/live-smoke.sh` as the reusable smoke runner for shipped CLI surfaces.

The script:
- builds `target/debug/idx` once by default
- isolates config, cache, and data under `tmp/live-smoke/<timestamp>/`
- records separate `.stdout` and `.stderr` files per case under `tmp/live-smoke/<timestamp>/logs/`, plus `.validation` diagnostics for JSON checks
- supports fast mock-backed runs and slower live-network passes; `jq` is required and provided by `nix develop`

## Common Runs

```bash
scripts/live-smoke.sh
scripts/live-smoke.sh --mode full
scripts/live-smoke.sh --mode mock
scripts/live-smoke.sh --group live-table --group live-json
scripts/live-smoke.sh --group live-nonfinite
scripts/live-smoke.sh --group cache --symbol BBRI
scripts/live-smoke.sh --dry-run --mode full
scripts/live-smoke.sh --bin ./tmp/release-install/bin/idx --no-build --mode mock
scripts/audit-msn-fundamentals.sh --tickers BUMI,ADRO,AIMS
```

## Nix CI Checks

`nix flake check -L` is the canonical local, pre-push, and Linux CI gate. Its
eleven checks cover:

- Rust formatting, Clippy (`--all-targets`), and all unit/CLI tests
- the optimized application build and verified crate packaging
- the packaged ownership publisher's `--help` entry point
- the offline mock CLI smoke matrix
- publisher regression scenarios, including the SQLite artifact builder
- installer regressions under `sh` and `dash`, plus installer and smoke-runner ShellCheck

All check tools are pinned by `flake.lock`. Installer fixtures use resolved
shell paths, and generated publisher stubs use the running Bash interpreter,
so the regression suites also work inside the Nix sandbox.

Crane shares dev-profile dependency artifacts between tests, Clippy, and
`cargo package` verification; the shipped application uses a separate release
dependency cache. Package verification still builds the packaged crate.
Integration-test edits do not invalidate the application build, and script
checks consume only their relevant source files. README/license edits affect
the packaging check, not the Rust test/application derivations.

CI cache keys include platform, dependency/toolchain inputs, and checked source
contents. Source changes can therefore save new outputs; restore prefixes first
reuse the same dependency/toolchain cache, then another cache for the platform.
The GitHub cache is opportunistic and subject to eviction, not a shared public
binary cache. Obsolete PR runs are cancelled; main-branch runs are not.

Use individual checks when investigating a failure (replace the system as
needed), or targeted Cargo commands inside the pinned development shell:

```bash
nix build .#checks.x86_64-linux.publisher-test -L
nix develop --command cargo test full_screener
```

The native macOS installer job remains separate to exercise BSD/macOS tools.
Live-network smoke is deliberately outside the sandboxed offline gate.

For a live full-list screener check, use an isolated config/cache and run:

```bash
idx -o json stocks screen --region us --filter top-performers --limit 600
idx --offline -o json stocks screen --region us --filter top-performers --limit 3
idx --offline -o json stocks screen --region us --filter top-performers --limit 25
```

The first command should not stop at the old 500-row cap when MSN reports more
candidates. The two offline results must match prefixes of the same cached full
list. Provider counts and prices vary; live HTTP failures are not offline-test
failures.

## Coverage Boundaries

The mock preset has 43 cases: 7 general, 19 stock, 9 cache, 2 routing, 4 error,
and 2 empty/unsupported ownership checks. This is a packaged-binary safety net,
not a claim that every important behavior is covered by shell smoke.

- All fifteen stock commands run in JSON mode with structural and fixture-value
  assertions. Four representative table renders check displayed data.
- JSON checks require exactly one document in the correct stream. Failed
  commands must leave stdout empty and expose the expected stable error code.
- Cache scenarios use zero TTL, force provider failures, and compare parsed
  offline/stale results with the warmed data; diagnostics must stay on stderr.
- Redundant table launches, duplicate routing cases, and repeated setup checks
  are omitted rather than counted as extra correctness coverage.

The CLI integration suite carries deeper workflows under the same Nix gate:
cache misses, provider isolation, filtered reports, non-finite values,
XLSX/ZIP import and ownership queries, snapshot install/no-op/force, and rejected
updates preserving database bytes and query results.

Screener regressions generate 503 distinct candidates, with ranking leaders
beyond the first 500. They exercise top/worst performers, volume, and market-cap
ordering, a small first request followed by complete offline reads, and rejected
missing/incomplete totals without cache poisoning. The raw fixture override
`IDX_MOCK_MSN_SCREENER_FIXTURE` is honored only with `IDX_USE_MOCK_PROVIDER`;
responses honor the requested batch size so the complete-fetch path is exercised.

Mocks do not verify current upstream availability, authentication, or response
drift. Live provider checks remain a separate, opt-in operation.



## Modes

- `live` runs the default real-network baseline: `general`, `live-table`, and `ownership`
- `mock` runs the deterministic baseline: `general`, `mock`, `cache`, `routing`, `errors`, and `ownership`
- `full` runs every group, including live JSON output checks and ownership-import verification

## Groups

- `general`: `version`, `completions`, `config`, and `cache` command basics
- `live-table`: all shipped `stocks` commands in live table mode
- `live-json`: all shipped `stocks` commands in live JSON mode
- `mock`: all shipped `stocks` commands against the mock provider in JSON mode, plus representative quote/history/fundamental/financials table renders
- `cache`: exact data equality across warm, `--offline`, and stale-cache fallback reads for quote, technical, and MSN `profile`
- `routing`: Yahoo/MSN provider routing plus explicit MSN history behavior
- `errors`: JSON error contract and invalid flag/input checks
- `live-nonfinite`: opt-in live MSN fundamentals checks for known non-finite ticker payloads (`BUMI`, `ADRO`, `AIMS`)
- `ownership`: safe ownership smoke checks that do not require imported ownership data
- `ownership-import`: live discovery/import hardening checks for supported `above1` import plus expected unsupported legacy-family failures

## Notes

- The runner forces `IDX_OUTPUT=table` as its default environment so table cases stay stable; JSON checks use `-o json` explicitly.
- Cache-group warm cases clear the smoke cache before they run so each warm/offline/stale sequence starts clean and stale-cache assertions are not masked by earlier groups.
- Use `--bin <path> --no-build` when you want to validate an installed binary instead of the workspace `target/debug/idx` build.
- Use `scripts/audit-msn-fundamentals.sh` for a full CLI valuation sweep across the IDX MSN symbol map. It is intentionally separate from the reusable smoke runner because it is a heavier provider-health audit, not a stable baseline check.
- Ownership commands that need imported data are intentionally not part of the baseline runner yet. The current baseline only covers `ownership releases` and the known unsupported `ownership import --fetch-bing`.
- `ownership sync` is still primarily covered by fixture-backed CLI tests rather than the reusable smoke runner.
- The new `ownership-import` group is intentionally opt-in for explicit `--group ownership-import` runs or `--mode full`; it discovers live URLs first, imports the supported `above1` attachment into the temp DB, then asserts the current `above5` and `investor-type` URLs fail with explicit unsupported-schema UX.
- The new `live-nonfinite` group is intentionally opt-in only. As of `2026-04-13`, the known real repro tickers are `BUMI`, `ADRO`, and `AIMS`.
- When a case fails, inspect its `.stdout`, `.stderr`, and (for JSON cases) `.validation` files in `tmp/live-smoke/.../logs/` before updating `TODO.md` or `FEATURE_SPEC.md`.
