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
scripts/live-smoke.sh --group cache --symbol BBRI
scripts/live-smoke.sh --dry-run --mode full
```

## Modes

- `live` runs the default real-network baseline: `general`, `live-table`, and `ownership`
- `mock` runs the deterministic baseline: `general`, `mock`, `cache`, `routing`, `errors`, and `ownership`
- `full` runs every group, including live JSON output checks

## Groups

- `general`: `version`, `completions`, `config`, and `cache` command basics
- `live-table`: all shipped `stocks` commands in live table mode
- `live-json`: all shipped `stocks` commands in live JSON mode
- `mock`: all shipped `stocks` commands against the mock provider in both table and JSON mode
- `cache`: cache warm, `--offline`, and stale-cache fallback checks for quote, technical, and MSN `profile`
- `routing`: Yahoo/MSN provider routing plus explicit MSN history unsupported behavior
- `errors`: JSON error contract and invalid flag/input checks
- `ownership`: safe ownership smoke checks that do not require imported ownership data

## Notes

- The runner forces `IDX_OUTPUT=table` as its default environment so table cases stay stable; JSON checks use `-o json` explicitly.
- Cache-group warm cases clear the smoke cache before they run so each warm/offline/stale sequence starts clean and stale-cache assertions are not masked by earlier groups.
- Ownership commands that need imported data are intentionally not part of the baseline runner yet. The current baseline only covers `ownership releases` and the known unsupported `ownership import --fetch-bing`.
- When a case fails, inspect the per-case log in `tmp/live-smoke/.../logs/` before updating `TODO.md` or `FEATURE_SPEC.md`.
