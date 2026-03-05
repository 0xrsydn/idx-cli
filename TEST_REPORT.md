# Unified CLI Test Report

Date: 2026-03-06

## Summary

- Unified CLI commands tested: `msn`, `news`, `export`, `extractor`
- Parser/validation tested with valid and invalid arguments
- Error handling standardized: command modules return errors; root handles exit code
- Remaining hard exit: only top-level `main()` calls `os.Exit(runRoot(...))`

## Command Matrix (Representative)

### Root

- `go run .` -> usage printed, exit `1`
- `go run . --help` -> usage printed, exit `1`
- `go run . unknown` -> usage + `unknown command`, exit `1`

### MSN

- `go run . msn --help` -> MSN usage, exit `1`
- `go run . msn badcmd` -> error unknown subcommand, exit `1`
- `go run . msn lookup BBCA TLKM XXXX` -> success, resolves BBCA/TLKM, unknown marked not found, exit `0`

#### `msn screener`
- `--help` -> screener usage, exit `0`
- `--region` (missing value) -> error, exit `1`
- `--limit nope` -> parse error, exit `1`
- `--filter invalid` -> validation error, exit `1`
- valid invocation attempted -> DNS failure to `assets.msn.com` in this environment, exit `1`

#### `msn fetch`
- `--help` -> fetch usage, exit `0`
- no id source -> validation error, exit `1`
- `--input` missing value -> error, exit `1`
- `--tickers` missing value -> error, exit `1`
- `--concurrency nope` -> parse error, exit `1`
- `--input` missing file -> FS error, exit `1`
- valid with tickers -> success, output `/tmp/fetch_tickers.json`, exit `0`
- valid with ids -> success, output `/tmp/fetch_ids.json`, exit `0`

#### `msn fetch-all`
- `--help` -> fetch-all usage, exit `0`
- `--db` missing value -> error, exit `1`
- `--delay oops` -> format error, exit `1`
- `--rps nope` -> parse error, exit `1`
- valid `--index weird --limit 1` -> fallback to all, success, `/tmp/fetchall_weird.db`, exit `0`
- valid `--index idx30 --limit 1` -> success, `/tmp/fetchall_idx30.db`, exit `0`

### News

- `go run . news --help` -> news usage, exit `1`
- valid news request attempted -> fails with missing `BRAVE_API_KEY` in current shell env, exit `1`
- `--from bad-date` -> parse error, exit `1`
- `--to bad-date` -> parse error, exit `1`
- `--count nope` -> parse error, exit `1`
- `--concurrency nope` -> parse error, exit `1`
- `--output` missing value -> error, exit `1`

### Export

- `go run . export --help` -> usage, exit `1`
- `go run . export badtarget` -> validation error, exit `1`
- `go run . export dashboard --db /tmp/fetchall_idx30.db --output /tmp/dashboard_refactor.xlsx` -> success, exit `0`
- `go run . export history --db /tmp/fetchall_idx30.db --output /tmp/history_refactor.xlsx` -> success, exit `0`

### Extractor

- `go run . extractor` -> usage, exit `1`
- `go run . extractor --help` -> usage, exit `1`

## Produced Artifacts

- `/tmp/fetch_one.json`
- `/tmp/fetch_tickers.json`
- `/tmp/fetch_ids.json`
- `/tmp/fetchall_weird.db`
- `/tmp/fetchall_idx30.db`
- `/tmp/dashboard_refactor.xlsx`
- `/tmp/history_refactor.xlsx`

## Notes

- Network/API behavior is environment-dependent (DNS and API key availability).
- `news` command requires `BRAVE_API_KEY` in the running shell environment.
- CLI behavior now consistently reports parse/validation/runtime errors without deep `log.Fatal` exits.
