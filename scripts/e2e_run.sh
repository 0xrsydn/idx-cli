#!/usr/bin/env bash
set -euo pipefail

APP_BIN="${1:-./bin/rubick}"
if [[ ! -x "$APP_BIN" ]]; then
  echo "error: binary not executable: $APP_BIN" >&2
  exit 1
fi

TS="$(date +%Y%m%d-%H%M%S)"
OUT="output/$TS"
mkdir -p "$OUT"

echo "timestamp=$TS" > "$OUT/RUN_INFO.txt"

echo "[1/5] msn fetch-all"
"$APP_BIN" msn fetch-all --index idx30 --limit 3 --db "$OUT/stocks.db" --rps 10 --delay 100-150 --concurrency 2

echo "[2/5] news plain"
"$APP_BIN" news IHSG --from 2026-03-01 --to 2026-03-05 --count 2 --concurrency 2 --output "$OUT/news_plain.json"

echo "[3/5] export simple csv"
"$APP_BIN" export simple --db "$OUT/stocks.db" --format csv --output "$OUT/csv"

echo "[4/5] export dashboard"
"$APP_BIN" export dashboard --db "$OUT/stocks.db" --output "$OUT/dashboard.xlsx"

echo "[5/5] export history"
"$APP_BIN" export history --db "$OUT/stocks.db" --output "$OUT/history.xlsx"

echo "E2E completed: $OUT"
