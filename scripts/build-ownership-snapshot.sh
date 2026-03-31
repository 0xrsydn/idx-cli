#!/usr/bin/env bash

set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/build-ownership-snapshot.sh --db <ownership.db> --output-dir <dir> [--base-url <url>]

Create a publishable ownership SQLite snapshot plus a manifest JSON file.

Options:
  --db <path>          Source ownership SQLite database
  --output-dir <dir>   Directory to write the copied snapshot and manifest
  --base-url <url>     Optional URL prefix to use for snapshot.download_url
  --help               Show this help
EOF
}

sha256_file() {
    local path="$1"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$path" | awk '{print $1}'
        return 0
    fi
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$path" | awk '{print $1}'
        return 0
    fi

    echo "sha256sum/shasum not found in PATH" >&2
    exit 1
}

DB_PATH=""
OUTPUT_DIR=""
BASE_URL=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --db)
            DB_PATH="${2:-}"
            shift 2
            ;;
        --output-dir)
            OUTPUT_DIR="${2:-}"
            shift 2
            ;;
        --base-url)
            BASE_URL="${2:-}"
            shift 2
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            echo "unknown argument: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [[ -z "$DB_PATH" || -z "$OUTPUT_DIR" ]]; then
    echo "--db and --output-dir are required" >&2
    usage >&2
    exit 2
fi

if [[ ! -f "$DB_PATH" ]]; then
    echo "ownership database not found: $DB_PATH" >&2
    exit 1
fi

if ! command -v sqlite3 >/dev/null 2>&1; then
    echo "sqlite3 is required to build ownership snapshots" >&2
    exit 1
fi

mkdir -p "$OUTPUT_DIR"

LATEST_AS_OF="$(sqlite3 "$DB_PATH" "SELECT as_of_date FROM ownership_releases ORDER BY as_of_date DESC, imported_at DESC LIMIT 1;")"
LATEST_RELEASE_SHA="$(sqlite3 "$DB_PATH" "SELECT sha256 FROM ownership_releases ORDER BY as_of_date DESC, imported_at DESC LIMIT 1;")"
LATEST_ROW_COUNT="$(sqlite3 "$DB_PATH" "SELECT row_count FROM ownership_releases ORDER BY as_of_date DESC, imported_at DESC LIMIT 1;")"
RELEASE_COUNT="$(sqlite3 "$DB_PATH" "SELECT COUNT(*) FROM ownership_releases;")"
TICKER_COUNT="$(sqlite3 "$DB_PATH" "SELECT COUNT(*) FROM tickers;")"

if [[ -z "$LATEST_AS_OF" || -z "$LATEST_RELEASE_SHA" ]]; then
    echo "ownership database has no imported releases: $DB_PATH" >&2
    exit 1
fi

ARTIFACT_NAME="ownership-snapshot-${LATEST_AS_OF}.sqlite"
ARTIFACT_PATH="$OUTPUT_DIR/$ARTIFACT_NAME"
MANIFEST_PATH="$OUTPUT_DIR/ownership-snapshot-manifest.json"

cp "$DB_PATH" "$ARTIFACT_PATH"

SQLITE_SHA256="$(sha256_file "$ARTIFACT_PATH")"
SIZE_BYTES="$(wc -c < "$ARTIFACT_PATH" | tr -d ' ')"
GENERATED_AT="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

if [[ -n "$BASE_URL" ]]; then
    DOWNLOAD_URL="${BASE_URL%/}/$ARTIFACT_NAME"
else
    DOWNLOAD_URL="$ARTIFACT_PATH"
fi

cat > "$MANIFEST_PATH" <<EOF
{
  "schema_version": 1,
  "generated_at": "$GENERATED_AT",
  "snapshot": {
    "kind": "sqlite",
    "compression": "none",
    "version": "$LATEST_AS_OF",
    "download_url": "$DOWNLOAD_URL",
    "sqlite_sha256": "$SQLITE_SHA256",
    "size_bytes": $SIZE_BYTES,
    "release_count": $RELEASE_COUNT,
    "latest_as_of_date": "$LATEST_AS_OF",
    "latest_release_sha256": "$LATEST_RELEASE_SHA",
    "latest_row_count": $LATEST_ROW_COUNT,
    "ticker_count": $TICKER_COUNT
  }
}
EOF

printf 'Wrote snapshot artifact: %s\n' "$ARTIFACT_PATH"
printf 'Wrote snapshot manifest: %s\n' "$MANIFEST_PATH"
