#!/usr/bin/env bash

set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/build-latest-ownership-snapshot.sh --output-dir <dir> [options]

Discover the latest supported IDX/KSEI above-1% report (XLSX from the IDX Data
Kepemilikan Saham page, or a legacy PDF announcement), import it into an isolated
ownership database, and emit GitHub-release-ready snapshot artifacts plus manifest.

Options:
  --idx-bin <path>       idx binary to use (default: ./target/debug/idx)
  --output-dir <dir>     Directory to write the copied snapshot and manifest
  --base-url <url>       Public URL prefix to use for snapshot.download_url
  --repo <owner/name>    GitHub repo used for the default base URL
                         (default: 0xrsydn/idx-cli)
  --release-tag <tag>    Stable GitHub release tag used for the default base URL
                         (default: ownership-snapshot-current)
  --history <n>          Also import the <n> previous monthly above-1% XLSX
                         reports (oldest first) so `ownership changes` works
                         from the snapshot (default: 0)
  --max-snapshot-bytes <n>
                         Refuse to emit a snapshot larger than this
                         (default: 10485760, the download limit of idx
                         clients <= v0.2.3; exceeding it breaks their sync)
  --keep-workdir         Keep the temp workdir instead of deleting it
  --help                 Show this help
EOF
}

IDX_BIN="./target/debug/idx"
OUTPUT_DIR=""
BASE_URL=""
REPO_FULL_NAME="0xrsydn/idx-cli"
RELEASE_TAG="ownership-snapshot-current"
KEEP_WORKDIR="0"
HISTORY="0"
MAX_SNAPSHOT_BYTES="10485760"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --idx-bin)
            IDX_BIN="${2:-}"
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
        --repo)
            REPO_FULL_NAME="${2:-}"
            shift 2
            ;;
        --release-tag)
            RELEASE_TAG="${2:-}"
            shift 2
            ;;
        --history)
            HISTORY="${2:-}"
            shift 2
            ;;
        --max-snapshot-bytes)
            MAX_SNAPSHOT_BYTES="${2:-}"
            shift 2
            ;;
        --keep-workdir)
            KEEP_WORKDIR="1"
            shift
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

if [[ -z "$OUTPUT_DIR" ]]; then
    echo "--output-dir is required" >&2
    usage >&2
    exit 2
fi

if ! [[ "$MAX_SNAPSHOT_BYTES" =~ ^[1-9][0-9]{0,17}$ ]]; then
    echo "--max-snapshot-bytes must be a positive integer" >&2
    exit 2
fi

if ! [[ "$HISTORY" =~ ^[0-9]+$ ]]; then
    echo "--history must be a non-negative integer" >&2
    exit 2
fi
# Strip leading zeros, then bound the value before arithmetic. A raw value
# with thousands of digits would wrap negative in $((10#$HISTORY)).
HISTORY_DIGITS="${HISTORY#"${HISTORY%%[!0]*}"}"
MAX_HISTORY=1000
if [[ -z "$HISTORY_DIGITS" ]]; then
    HISTORY=0
else
    if (( ${#HISTORY_DIGITS} > 6 )); then
        echo "--history must be at most $MAX_HISTORY" >&2
        exit 2
    fi
    HISTORY=$((10#$HISTORY_DIGITS))
    if (( HISTORY > MAX_HISTORY )); then
        echo "--history must be at most $MAX_HISTORY" >&2
        exit 2
    fi
fi

if [[ -z "$BASE_URL" ]]; then
    BASE_URL="https://github.com/${REPO_FULL_NAME}/releases/download/${RELEASE_TAG}"
fi

if ! command -v jq >/dev/null 2>&1; then
    echo "jq is required for manifest/source metadata processing" >&2
    exit 1
fi

if ! "$IDX_BIN" version >/dev/null 2>&1; then
    echo "failed to run idx binary: $IDX_BIN" >&2
    echo "build the CLI first or pass --idx-bin <path>" >&2
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BASIC_BUILDER="$SCRIPT_DIR/build-ownership-snapshot.sh"
if [[ ! -x "$BASIC_BUILDER" ]]; then
    echo "required helper script is missing or not executable: $BASIC_BUILDER" >&2
    exit 1
fi

WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/idx-ownership-snapshot.XXXXXX")"
cleanup() {
    if [[ "$KEEP_WORKDIR" == "1" ]]; then
        printf 'Kept workdir: %s\n' "$WORKDIR"
        return
    fi
    rm -rf "$WORKDIR"
}
trap cleanup EXIT

export XDG_DATA_HOME="$WORKDIR/data"
export XDG_CACHE_HOME="$WORKDIR/cache"
export XDG_CONFIG_HOME="$WORKDIR/config"
mkdir -p "$XDG_DATA_HOME" "$XDG_CACHE_HOME" "$XDG_CONFIG_HOME"

DISCOVERY_JSON="$WORKDIR/discovery.json"
RELEASES_JSON="$WORKDIR/releases.json"
MANIFEST_PATH="$OUTPUT_DIR/ownership-snapshot-manifest.json"
DB_PATH="$XDG_DATA_HOME/idx/ownership.db"

printf 'Discovering latest supported IDX/KSEI above-1%% report...\n'
"$IDX_BIN" -o json ownership discover --family above1 --limit 50 > "$DISCOVERY_JSON"

discovery_payload="$(
    jq -r '
        if type != "array" or length == 0 then
            error("ownership discover returned no reports")
        else
            .[0]
            | if .status != "supported" then
                error("latest discovered report is not supported: \(.status // "null")")
              else .
              end
            | [
                (.family // error("discovered report is missing family")),
                (.listing_page_url // error("discovered report is missing listing_page_url")),
                (.query_url // error("discovered report is missing query_url")),
                (.pdf_url // error("discovered report is missing pdf_url")),
                (.title // error("discovered report is missing title")),
                (.publish_date // error("discovered report is missing publish_date")),
                (.original_filename // "")
              ]
            | .[]
        end
    ' "$DISCOVERY_JSON"
)"
mapfile -t discovery_fields <<< "$discovery_payload"

DISCOVERED_FAMILY="${discovery_fields[0]:-}"
DISCOVERED_LISTING_PAGE_URL="${discovery_fields[1]:-}"
DISCOVERED_QUERY_URL="${discovery_fields[2]:-}"
DISCOVERED_PDF_URL="${discovery_fields[3]:-}"
DISCOVERED_TITLE="${discovery_fields[4]:-}"
DISCOVERED_PUBLISH_DATE="${discovery_fields[5]:-}"
DISCOVERED_ORIGINAL_FILENAME="${discovery_fields[6]:-}"

if (( HISTORY > 0 )); then
    # Earlier supported XLSX months, one per as-of date, newest first; import oldest first.
    # Run jq in a plain command substitution (not <(...)) so a jq failure
    # stops the script under set -e instead of looking like zero months.
    history_payload="$(
        jq -r --arg latest "$DISCOVERED_PDF_URL" --argjson n "$HISTORY" '
            (map(select(.pdf_url == $latest)) | .[0].as_of_date // "") as $latest_as_of
            | [ .[]
                | select(.status == "supported" and .format == "xlsx" and .as_of_date != null)
                | select($latest_as_of == "" or .as_of_date < $latest_as_of) ]
            | unique_by(.as_of_date)
            | sort_by(.as_of_date) | reverse
            | .[:$n] | reverse
            | .[].pdf_url
        ' "$DISCOVERY_JSON"
    )"
    history_urls=()
    if [[ -n "$history_payload" ]]; then
        mapfile -t history_urls <<< "$history_payload"
    fi
    printf 'Importing %d earlier monthly report(s) for history...\n' "${#history_urls[@]}"
    for url in "${history_urls[@]}"; do
        "$IDX_BIN" ownership import --url "$url"
    done
fi

printf 'Importing discovered report into isolated DB...\n'
"$IDX_BIN" ownership import --url "$DISCOVERED_PDF_URL"

printf 'Inspecting imported release metadata...\n'
"$IDX_BIN" -o json ownership releases > "$RELEASES_JSON"

release_payload="$(
    jq -r --arg expected_source "$DISCOVERED_PDF_URL" '
        if type != "array" or length == 0 then
            error("ownership releases returned no imported releases")
        else
            .[0]
            | if (.source_url // "") != $expected_source then
                error(
                    "latest imported release source_url mismatch: \(.source_url // "null") != \($expected_source)"
                )
              else .
              end
            | [
                (.as_of_date // error("latest imported release is missing as_of_date")),
                (
                    (.sha256 // "")
                    | if test("^[0-9A-Fa-f]{64}$") then .
                      else error("latest imported release has invalid sha256")
                      end
                ),
                (
                    (.row_count // 0)
                    | if type == "number" and . > 0 then tostring
                      else error("latest imported release has invalid row_count")
                      end
                )
              ]
            | .[]
        end
    ' "$RELEASES_JSON"
)"
mapfile -t release_fields <<< "$release_payload"

IMPORTED_AS_OF_DATE="${release_fields[0]:-}"
IMPORTED_RELEASE_SHA256="${release_fields[1]:-}"
IMPORTED_ROW_COUNT="${release_fields[2]:-}"

printf 'Building snapshot artifact and manifest...\n'
"$BASIC_BUILDER" --db "$DB_PATH" --output-dir "$OUTPUT_DIR" --base-url "$BASE_URL"

# Released clients cannot download a snapshot above their HTTP body limit, so
# publishing one would break `idx ownership sync` for every existing install.
SNAPSHOT_BYTES="$(jq -r '.snapshot.size_bytes // empty' "$MANIFEST_PATH")"
if ! [[ "$SNAPSHOT_BYTES" =~ ^[0-9]+$ ]]; then
    echo "built manifest has no valid snapshot.size_bytes" >&2
    exit 1
fi
if (( SNAPSHOT_BYTES > MAX_SNAPSHOT_BYTES )); then
    printf 'snapshot is %s bytes (%s monthly releases), above --max-snapshot-bytes %s; lower --history\n' \
        "$SNAPSHOT_BYTES" "$(jq -r '.snapshot.release_count' "$MANIFEST_PATH")" "$MAX_SNAPSHOT_BYTES" >&2
    exit 1
fi

TMP_MANIFEST_PATH="$WORKDIR/ownership-snapshot-manifest.json"
jq \
    --arg family "$DISCOVERED_FAMILY" \
    --arg listing_page_url "$DISCOVERED_LISTING_PAGE_URL" \
    --arg query_url "$DISCOVERED_QUERY_URL" \
    --arg pdf_url "$DISCOVERED_PDF_URL" \
    --arg title "$DISCOVERED_TITLE" \
    --arg publish_date "$DISCOVERED_PUBLISH_DATE" \
    --arg original_filename "$DISCOVERED_ORIGINAL_FILENAME" \
    '
        .source = {
            family: $family,
            listing_page_url: $listing_page_url,
            query_url: $query_url,
            pdf_url: $pdf_url,
            title: $title,
            publish_date: $publish_date,
            original_filename: ($original_filename | if . == "" then null else . end)
        }
    ' "$MANIFEST_PATH" > "$TMP_MANIFEST_PATH"
mv "$TMP_MANIFEST_PATH" "$MANIFEST_PATH"

printf 'Prepared release-ready snapshot from %s (%s)\n' \
    "$DISCOVERED_PDF_URL" "$IMPORTED_AS_OF_DATE"
printf 'Manifest URL target: %s/ownership-snapshot-manifest.json\n' "$BASE_URL"
printf 'SQLite URL target: %s\n' "$(jq -r '.snapshot.download_url // empty' "$MANIFEST_PATH")"
