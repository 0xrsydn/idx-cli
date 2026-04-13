#!/usr/bin/env bash

set -u
set -o pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_PATH=""
WORKDIR=""
TICKERS=""
BUILD=1

CONFIG_HOME=""
CACHE_HOME=""
DATA_HOME=""
LOG_DIR=""
SUMMARY_FILE=""

declare -a BASE_ENV=()
declare -a TICKER_LIST=()

usage() {
    cat <<'EOF'
Usage: scripts/audit-msn-fundamentals.sh [options]

Run `idx -o json stocks valuation <ticker>` across the IDX MSN symbol map.
This is a heavier provider audit, not a default smoke check.

Options:
  --bin <path>            Use an existing idx binary instead of target/debug/idx
  --no-build              Skip cargo build
  --workdir <path>        Artifact root. Default: tmp/msn-fundamentals-audit/<timestamp>
  --tickers <list>        Comma-separated ticker override instead of the full symbol map
  --help                  Show this help

Examples:
  scripts/audit-msn-fundamentals.sh
  scripts/audit-msn-fundamentals.sh --tickers BUMI,ADRO,AIMS
  scripts/audit-msn-fundamentals.sh --bin ./target/debug/idx --no-build
EOF
}

parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --bin)
                BIN_PATH="${2:-}"
                BUILD=0
                shift 2
                ;;
            --no-build)
                BUILD=0
                shift
                ;;
            --workdir)
                WORKDIR="${2:-}"
                shift 2
                ;;
            --tickers)
                TICKERS="${2:-}"
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
}

prepare_paths() {
    if [[ -z "$WORKDIR" ]]; then
        WORKDIR="$ROOT_DIR/tmp/msn-fundamentals-audit/$(date +%Y%m%d-%H%M%S)"
    fi

    CONFIG_HOME="$WORKDIR/config"
    CACHE_HOME="$WORKDIR/cache"
    DATA_HOME="$WORKDIR/data"
    LOG_DIR="$WORKDIR/logs"
    SUMMARY_FILE="$WORKDIR/summary.tsv"

    mkdir -p "$CONFIG_HOME" "$CACHE_HOME" "$DATA_HOME" "$LOG_DIR"

    BASE_ENV=(
        "XDG_CONFIG_HOME=$CONFIG_HOME"
        "XDG_CACHE_HOME=$CACHE_HOME"
        "XDG_DATA_HOME=$DATA_HOME"
        "IDX_PROVIDER=msn"
        "IDX_HISTORY_PROVIDER=auto"
    )
}

build_binary() {
    if [[ -z "$BIN_PATH" ]]; then
        BIN_PATH="$ROOT_DIR/target/debug/idx"
    fi

    if (( BUILD )); then
        (
            cd "$ROOT_DIR" || exit 1
            cargo build --quiet --bin idx
        ) || {
            echo "build failed" >&2
            exit 1
        }
    fi

    if [[ ! -x "$BIN_PATH" ]]; then
        echo "idx binary not found or not executable: $BIN_PATH" >&2
        exit 1
    fi
}

resolve_tickers() {
    local raw_ticker
    local old_ifs

    if [[ -n "$TICKERS" ]]; then
        old_ifs="$IFS"
        IFS=','
        for raw_ticker in $TICKERS; do
            raw_ticker="${raw_ticker//[[:space:]]/}"
            if [[ -n "$raw_ticker" ]]; then
                TICKER_LIST+=("$raw_ticker")
            fi
        done
        IFS="$old_ifs"
    else
        mapfile -t TICKER_LIST < <(cut -f1 "$ROOT_DIR/src/api/msn/symbol_ids.tsv")
    fi

    if [[ ${#TICKER_LIST[@]} -eq 0 ]]; then
        echo "no tickers selected" >&2
        exit 1
    fi
}

run_audit() {
    local total="${#TICKER_LIST[@]}"
    local index=0
    local passed=0
    local failed=0
    local ticker
    local log_file
    local message

    printf 'ticker\tstatus\tmessage\tlog\n' >"$SUMMARY_FILE"

    for ticker in "${TICKER_LIST[@]}"; do
        index=$((index + 1))
        log_file="$LOG_DIR/${ticker}.log"

        printf '[%04d/%04d] %s\n' "$index" "$total" "$ticker"

        if env "${BASE_ENV[@]}" "$BIN_PATH" -q -o json stocks valuation "$ticker" >"$log_file" 2>&1 \
            && grep -q '"overall_signal"' "$log_file"; then
            passed=$((passed + 1))
            printf '%s\tok\t-\t%s\n' "$ticker" "$log_file" >>"$SUMMARY_FILE"
        else
            failed=$((failed + 1))
            message="$(tr '\n' ' ' <"$log_file" | tr '\t' ' ' | cut -c1-240)"
            printf '%s\tfailed\t%s\t%s\n' "$ticker" "$message" "$log_file" >>"$SUMMARY_FILE"
        fi
    done

    printf '\nresults: passed=%d failed=%d total=%d\n' "$passed" "$failed" "$total"
    printf 'summary: %s\n' "$SUMMARY_FILE"

    if (( failed > 0 )); then
        exit 1
    fi
}

main() {
    parse_args "$@"
    prepare_paths
    build_binary
    resolve_tickers
    run_audit
}

main "$@"
