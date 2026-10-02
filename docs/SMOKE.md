# Smoke Checks

Use `scripts/live-smoke.sh` as the reusable smoke runner for shipped CLI surfaces.

The script:
- builds `target/debug/idx` once by default
- isolates config, cache, and data under `tmp/live-smoke/<timestamp>/`
- records one log file per case under `tmp/live-smoke/<timestamp>/logs/`
- supports fast mock-backed runs and slower live-network passes

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

`nix flake check -L` runs formatting, Clippy, Rust tests, crate packaging, the
application and ownership-publisher builds, and the offline mock smoke. Crane
shares dependency build artifacts between the Rust derivations. CI restores the
Nix store from the GitHub Actions cache, keyed on the flake and Cargo inputs.

The publisher regression script remains a separate CI step:

```bash
scripts/publish-ownership-snapshot-test.sh
```

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


## Modes

- `live` runs the default real-network baseline: `general`, `live-table`, and `ownership`
- `mock` runs the deterministic baseline: `general`, `mock`, `cache`, `routing`, `errors`, and `ownership`
- `full` runs every group, including live JSON output checks and ownership-import verification

## Groups

- `general`: `version`, `completions`, `config`, and `cache` command basics
- `live-table`: all shipped `stocks` commands in live table mode
- `live-json`: all shipped `stocks` commands in live JSON mode
- `mock`: all shipped `stocks` commands against the mock provider in both table and JSON mode
- `cache`: cache warm, `--offline`, and stale-cache fallback checks for quote, technical, and MSN `profile`
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
- When a case fails, inspect the per-case log in `tmp/live-smoke/.../logs/` before updating `TODO.md` or `FEATURE_SPEC.md`.
