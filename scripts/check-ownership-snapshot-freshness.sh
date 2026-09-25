#!/usr/bin/env bash
# Fail when the published ownership snapshot is older than expected, so a
# publish job that keeps failing (or keeps finding nothing new) gets noticed.

set -euo pipefail

usage() {
    cat <<'USAGE'
Usage: scripts/check-ownership-snapshot-freshness.sh [options]

Options:
  --manifest-url <url>   Published manifest (default: the ownership-snapshot-current
                         release of 0xrsydn/idx-cli)
  --max-age-days <n>     Allowed age of latest_as_of_date in days (default: 40;
                         the source is monthly and lands 2-3 days after month end)
  --now <YYYY-MM-DD>     Reference date instead of today (for testing)
  --help                 Show this help

Exit status: 0 fresh, 1 stale, 2 usage error, 3 manifest unreadable.
USAGE
}

MANIFEST_URL="https://github.com/0xrsydn/idx-cli/releases/download/ownership-snapshot-current/ownership-snapshot-manifest.json"
MAX_AGE_DAYS="40"
NOW="$(date -u +%F)"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --manifest-url) MANIFEST_URL="${2:-}"; shift 2 ;;
        --max-age-days) MAX_AGE_DAYS="${2:-}"; shift 2 ;;
        --now) NOW="${2:-}"; shift 2 ;;
        --help|-h) usage; exit 0 ;;
        *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done

if ! [[ "$MAX_AGE_DAYS" =~ ^[0-9]+$ ]]; then
    echo "--max-age-days must be a non-negative integer" >&2
    exit 2
fi
if ! [[ "$NOW" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] || ! date -u -d "$NOW" +%s >/dev/null 2>&1; then
    echo "--now must be a YYYY-MM-DD date" >&2
    exit 2
fi

if [[ "$MANIFEST_URL" =~ ^https?:// ]]; then
    manifest="$(curl --fail --silent --show-error --location --retry 3 "$MANIFEST_URL")" || {
        echo "FRESHNESS: unreadable manifest $MANIFEST_URL" >&2
        exit 3
    }
else
    manifest="$(cat "$MANIFEST_URL")" || exit 3
fi

as_of="$(jq -r '.snapshot.latest_as_of_date // empty' <<< "$manifest" 2>/dev/null || true)"
if ! [[ "$as_of" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] || ! date -u -d "$as_of" +%s >/dev/null 2>&1; then
    echo "FRESHNESS: manifest has no valid snapshot.latest_as_of_date ('$as_of')" >&2
    exit 3
fi

age_days=$(( ($(date -u -d "$NOW" +%s) - $(date -u -d "$as_of" +%s)) / 86400 ))
if (( age_days > 10#$MAX_AGE_DAYS )); then
    printf 'FRESHNESS: stale latest_as_of_date=%s age_days=%d max=%s\n' "$as_of" "$age_days" "$MAX_AGE_DAYS" >&2
    exit 1
fi
printf 'FRESHNESS: ok latest_as_of_date=%s age_days=%d max=%s\n' "$as_of" "$age_days" "$MAX_AGE_DAYS"
