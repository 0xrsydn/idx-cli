#!/usr/bin/env bash

set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/build-latest-ownership-snapshot.sh --output-dir <dir> [options]

Discover the latest supported IDX/KSEI ownership PDF, import it into an isolated
ownership database, and emit GitHub-release-ready snapshot artifacts plus manifest.

Options:
  --idx-bin <path>       idx binary to use (default: ./target/debug/idx)
  --output-dir <dir>     Directory to write the copied snapshot and manifest
  --base-url <url>       Public URL prefix to use for snapshot.download_url
  --repo <owner/name>    GitHub repo used for the default base URL
                         (default: 0xrsydn/idx-cli)
  --release-tag <tag>    Stable GitHub release tag used for the default base URL
                         (default: ownership-snapshot-current)
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

printf 'Discovering latest supported IDX/KSEI ownership PDF...\n'
"$IDX_BIN" -o json ownership discover --family above1 --limit 1 > "$DISCOVERY_JSON"

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

printf 'Importing discovered PDF into isolated DB...\n'
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
printf 'SQLite URL target: %s/ownership-snapshot-%s.sqlite\n' "$BASE_URL" "$IMPORTED_AS_OF_DATE"
