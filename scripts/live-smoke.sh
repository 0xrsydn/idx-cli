#!/usr/bin/env bash
# Live provider drift check: runs shipped commands against real MSN, Yahoo,
# and IDX endpoints and validates the JSON output shape. Deterministic CLI
# behavior is covered by tests/cli.rs; this only answers "do the upstreams
# still look the way our parsers expect?"

set -u
set -o pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SYMBOL="BBCA"
BIN_PATH=""
DRY_RUN=0
declare -a GROUPS_SELECTED=()
declare -a FAILURES=()
TOTAL=0
PASSED=0

usage() {
    cat <<'EOF'
Usage: scripts/live-smoke.sh [options]

Options:
  --group <stocks|ownership>  Restrict to a group. Repeatable. Default: both
  --symbol <ticker>           Primary ticker. Default: BBCA
  --bin <path>                Use an existing idx binary (skips cargo build)
  --dry-run                   Print cases without running them
  --help                      Show this help
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --group) GROUPS_SELECTED+=("${2:-}"); shift 2 ;;
        --symbol) SYMBOL="${2:-}"; shift 2 ;;
        --bin) BIN_PATH="${2:-}"; shift 2 ;;
        --dry-run) DRY_RUN=1; shift ;;
        --help|-h) usage; exit 0 ;;
        *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
    esac
done
[[ ${#GROUPS_SELECTED[@]} -eq 0 ]] && GROUPS_SELECTED=(stocks ownership)

for group in "${GROUPS_SELECTED[@]}"; do
    case "$group" in
        stocks|ownership) ;;
        *) echo "invalid --group '$group' (expected stocks or ownership)" >&2; exit 2 ;;
    esac
done

selected() {
    local group
    for group in "${GROUPS_SELECTED[@]}"; do
        [[ "$group" == "$1" ]] && return 0
    done
    return 1
}

WORKDIR="$ROOT_DIR/tmp/live-smoke/$(date +%Y%m%d-%H%M%S)"
LOG_DIR="$WORKDIR/logs"
BASE_ENV=(
    "XDG_CONFIG_HOME=$WORKDIR/config"
    "XDG_CACHE_HOME=$WORKDIR/cache"
    "XDG_DATA_HOME=$WORKDIR/data"
    "IDX_PROVIDER=msn"
    "IDX_HISTORY_PROVIDER=auto"
    "IDX_NO_COLOR=1"
)

# jq filter describing the JSON shape each command must keep.
contract() {
    case "$1" in
        "stocks quote "*|"stocks screen"*)
            echo 'type == "array" and length > 0 and all(.[]; (.symbol | type == "string") and (.price | type == "number"))' ;;
        "stocks history "*)
            echo 'type == "array" and length > 0 and all(.[]; (.date | type == "string") and (.close | type == "number"))' ;;
        "stocks technical "*)
            echo '(.current_price | type == "number") and (.signals.overall | type == "string")' ;;
        "stocks growth "*|"stocks valuation "*|"stocks risk "*)
            echo '.overall_signal | type == "string"' ;;
        "stocks fundamental "*)
            echo '(.growth.overall_signal | type == "string") and (.valuation.overall_signal | type == "string") and (.risk.overall_signal | type == "string")' ;;
        "stocks compare "*)
            echo 'type == "array" and length > 0 and all(.[]; (.symbol | type == "string") and (.valuation | type == "object"))' ;;
        "stocks profile "*)
            echo '(.symbol | type == "string") and has("long_name") and (.officers | type == "array")' ;;
        "stocks financials "*)
            echo '(.instrument.symbol | type == "string") and has("income_statement") and has("balance_sheet") and has("cash_flow")' ;;
        "stocks earnings "*)
            echo '(.history | type == "array") and (.forecast | type == "array")' ;;
        "stocks sentiment "*)
            echo '.statistics | type == "array"' ;;
        "stocks insights "*)
            echo '(.highlights | type == "array") and (.risks | type == "array")' ;;
        "stocks news "*)
            echo 'type == "array" and all(.[]; (.title | type == "string") and (.url | type == "string"))' ;;
        "ownership discover"*)
            echo 'type == "array" and length > 0 and (.[0].pdf_url | type == "string")' ;;
        "ownership releases")
            echo 'type == "array" and length == 1' ;;
        *)
            echo 'true' ;;
    esac
}

run_case() {
    local name="$1" cmd="$2" filter log
    local -a args
    TOTAL=$((TOTAL + 1))
    printf '[%02d] %s\n' "$TOTAL" "$name"
    if (( DRY_RUN )); then
        printf '     idx -o json %s\n' "$cmd"
        return
    fi

    log="$LOG_DIR/$(printf '%02d' "$TOTAL")_$name"
    read -r -a args <<< "$cmd"
    if ! env "${BASE_ENV[@]}" "$BIN_PATH" -o json "${args[@]}" >"$log.stdout" 2>"$log.stderr"; then
        FAILURES+=("$name: non-zero exit (see $log.stderr)")
        echo "     FAIL exit"
        return
    fi
    filter="$(contract "$cmd")"
    if ! jq -e -s "length == 1 and (.[0] | $filter)" "$log.stdout" >/dev/null 2>"$log.validation"; then
        FAILURES+=("$name: output shape changed (see $log.stdout)")
        echo "     FAIL shape"
        return
    fi
    PASSED=$((PASSED + 1))
    echo "     PASS"
}

if (( ! DRY_RUN )); then
    command -v jq >/dev/null 2>&1 || { echo "jq is required; run inside nix develop" >&2; exit 1; }
    mkdir -p "$LOG_DIR"
    if [[ -z "$BIN_PATH" ]]; then
        BIN_PATH="$ROOT_DIR/target/debug/idx"
        (cd "$ROOT_DIR" && cargo build --quiet --bin idx) || { echo "cargo build failed" >&2; exit 1; }
    fi
    env "${BASE_ENV[@]}" "$BIN_PATH" config set ownership.db_path "$WORKDIR/ownership.db" >/dev/null
fi

if selected stocks; then
    for cmd in quote technical growth valuation risk fundamental profile financials earnings sentiment insights; do
        run_case "$cmd" "stocks $cmd $SYMBOL"
    done
    run_case history "stocks history $SYMBOL --period 3mo"
    run_case news "stocks news $SYMBOL --limit 5"
    run_case screen "stocks screen --filter top-performers --limit 10"
    run_case compare "stocks compare $SYMBOL,BBRI"
    # BUMI/ADRO/AIMS return non-finite MSN key ratios (Infinity/NaN).
    run_case compare-nonfinite "stocks compare BUMI,ADRO,AIMS"
fi

if selected ownership; then
    run_case discover "ownership discover --limit 1"
    if (( ! DRY_RUN )) && [[ -s "$LOG_DIR/$(printf '%02d' "$TOTAL")_discover.stdout" ]]; then
        url="$(jq -r '.[0].pdf_url' "$LOG_DIR/$(printf '%02d' "$TOTAL")_discover.stdout")"
        run_case import "ownership import --url $url"
        run_case releases "ownership releases"
    fi
fi

(( DRY_RUN )) && { echo "dry-run: $TOTAL case(s)"; exit 0; }
printf '\nsummary: %d passed, %d failed\nlogs: %s\n' "$PASSED" "$((TOTAL - PASSED))" "$LOG_DIR"
if (( ${#FAILURES[@]} > 0 )); then
    printf '  - %s\n' "${FAILURES[@]}"
    exit 1
fi
