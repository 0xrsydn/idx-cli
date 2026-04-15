#!/usr/bin/env bash

set -u
set -o pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODE="live"
BIN_PATH=""
WORKDIR=""
SYMBOL="BBCA"
COMPARE_SYMBOLS="BBCA,BBRI"
NEWS_LIMIT="5"
SCREEN_LIMIT="10"
DRY_RUN=0
BUILD=1

declare -a GROUP_FILTERS=()
declare -a CASES=()
declare -a FAILURES=()

TOTAL_CASES=0
PASSED_CASES=0
FAILED_CASES=0
SEP=$'\x1f'

CONFIG_HOME=""
CACHE_HOME=""
DATA_HOME=""
OWNERSHIP_DB=""
LOG_DIR=""
OWNERSHIP_ABOVE1_URL=""
OWNERSHIP_ABOVE5_URL=""
OWNERSHIP_INVESTOR_TYPE_URL=""

declare -a BASE_ENV=()

usage() {
    cat <<'EOF'
Usage: scripts/live-smoke.sh [options]

Reusable smoke runner for shipped idx-cli command surfaces.

Options:
  --mode <live|mock|full>   Preset case selection. Default: live
  --group <name>            Restrict to one or more groups. Repeatable.
  --symbol <ticker>         Primary ticker for stock cases. Default: BBCA
  --compare-symbols <list>  Compare symbols. Default: BBCA,BBRI
  --news-limit <n>          News limit. Default: 5
  --screen-limit <n>        Screener limit. Default: 10
  --workdir <path>          Artifact root. Default: tmp/live-smoke/<timestamp>
  --bin <path>              Use an existing idx binary instead of target/debug/idx
  --no-build                Skip cargo build
  --dry-run                 Print selected cases without running them
  --help                    Show this help

Groups:
  general      version/completions/config/cache basics
  live-table   live network stock commands in table mode
  live-json    live network stock commands in JSON mode
  mock         mock-backed stock commands in table + JSON mode
  cache        deterministic cache/offline/stale-cache checks
  routing      provider routing and explicit unsupported checks
  errors       JSON error contract and invalid-flag checks
  live-nonfinite  opt-in live MSN fundamentals checks for known non-finite tickers
  ownership    ownership commands that are safe without imported data
  ownership-import  live ownership discovery/import hardening checks

Examples:
  scripts/live-smoke.sh
  scripts/live-smoke.sh --mode full
  scripts/live-smoke.sh --mode mock --group cache
  scripts/live-smoke.sh --group live-table --group live-json --symbol BBRI
  scripts/live-smoke.sh --group live-nonfinite
EOF
}

add_group_filters() {
    local raw="$1"
    local part
    local old_ifs="$IFS"
    IFS=','
    for part in $raw; do
        if [[ -n "$part" ]]; then
            GROUP_FILTERS+=("$part")
        fi
    done
    IFS="$old_ifs"
}

mode_allows_group() {
    local group="$1"
    case "$MODE" in
        live)
            case "$group" in
                general|live-table|ownership) return 0 ;;
                *) return 1 ;;
            esac
            ;;
        mock)
            case "$group" in
                general|mock|cache|routing|errors|ownership) return 0 ;;
                *) return 1 ;;
            esac
            ;;
        full)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

group_requested() {
    local group="$1"
    local selected

    if [[ ${#GROUP_FILTERS[@]} -eq 0 ]]; then
        return 0
    fi

    for selected in "${GROUP_FILTERS[@]}"; do
        if [[ "$selected" == "$group" ]]; then
            return 0
        fi
    done

    return 1
}

should_run_group() {
    local group="$1"
    if [[ ${#GROUP_FILTERS[@]} -gt 0 ]]; then
        group_requested "$group"
        return
    fi

    mode_allows_group "$group"
}

add_case() {
    CASES+=("$1$SEP$2$SEP$3$SEP$4$SEP$5$SEP$6")
}

register_cases() {
    local live_env="IDX_PROVIDER=msn IDX_HISTORY_PROVIDER=auto"
    local mock_env="IDX_PROVIDER=msn IDX_HISTORY_PROVIDER=auto IDX_USE_MOCK_PROVIDER=1"
    local yahoo_mock_env="IDX_PROVIDER=yahoo IDX_HISTORY_PROVIDER=auto IDX_USE_MOCK_PROVIDER=1"
    local quote_zero_ttl_env="IDX_PROVIDER=msn IDX_USE_MOCK_PROVIDER=1 IDX_CACHE_QUOTE_TTL=0"
    local profile_zero_ttl_env="IDX_PROVIDER=msn IDX_USE_MOCK_PROVIDER=1 IDX_CACHE_FUNDAMENTAL_TTL=0"
    local live_symbol="$SYMBOL"
    local compare_args="$COMPARE_SYMBOLS"
    local history_args="stocks history $live_symbol --period 3mo"
    local screen_args="stocks screen --filter top-performers --limit $SCREEN_LIMIT"
    local news_args="stocks news $live_symbol --limit $NEWS_LIMIT"

    add_case "general" "version" "0" "" "version" ""
    add_case "general" "completions-bash" "0" "" "completions bash" ""
    add_case "general" "completions-zsh" "0" "" "completions zsh" ""
    add_case "general" "completions-fish" "0" "" "completions fish" ""
    add_case "general" "config-path" "0" "" "config path" ""
    add_case "general" "config-init" "0" "" "config init" ""
    add_case "general" "config-set-output-json" "0" "" "config set general.output json" ""
    add_case "general" "config-get-output" "0" "" "config get general.output" "json"
    add_case "general" "config-set-output-table" "0" "" "config set general.output table" ""
    add_case "general" "config-set-db-path" "0" "" "config set ownership.db_path $OWNERSHIP_DB" ""
    add_case "general" "config-get-db-path" "0" "" "config get ownership.db_path" "$OWNERSHIP_DB"
    add_case "general" "cache-info" "0" "" "cache info" ""
    add_case "general" "cache-clear" "0" "" "cache clear" ""

    add_case "live-table" "quote" "0" "$live_env" "stocks quote $live_symbol" ""
    add_case "live-table" "history" "0" "$live_env" "$history_args" ""
    add_case "live-table" "technical" "0" "$live_env" "stocks technical $live_symbol" ""
    add_case "live-table" "growth" "0" "$live_env" "stocks growth $live_symbol" ""
    add_case "live-table" "valuation" "0" "$live_env" "stocks valuation $live_symbol" ""
    add_case "live-table" "risk" "0" "$live_env" "stocks risk $live_symbol" ""
    add_case "live-table" "fundamental" "0" "$live_env" "stocks fundamental $live_symbol" ""
    add_case "live-table" "compare" "0" "$live_env" "stocks compare $compare_args" ""
    add_case "live-table" "profile" "0" "$live_env" "stocks profile $live_symbol" ""
    add_case "live-table" "financials" "0" "$live_env" "stocks financials $live_symbol" ""
    add_case "live-table" "earnings" "0" "$live_env" "stocks earnings $live_symbol" ""
    add_case "live-table" "sentiment" "0" "$live_env" "stocks sentiment $live_symbol" ""
    add_case "live-table" "insights" "0" "$live_env" "stocks insights $live_symbol" ""
    add_case "live-table" "news" "0" "$live_env" "$news_args" ""
    add_case "live-table" "screen" "0" "$live_env" "$screen_args" ""

    add_case "live-json" "quote" "0" "$live_env" "-o json stocks quote $live_symbol" ""
    add_case "live-json" "history" "0" "$live_env" "-o json $history_args" ""
    add_case "live-json" "technical" "0" "$live_env" "-o json stocks technical $live_symbol" ""
    add_case "live-json" "growth" "0" "$live_env" "-o json stocks growth $live_symbol" ""
    add_case "live-json" "valuation" "0" "$live_env" "-o json stocks valuation $live_symbol" ""
    add_case "live-json" "risk" "0" "$live_env" "-o json stocks risk $live_symbol" ""
    add_case "live-json" "fundamental" "0" "$live_env" "-o json stocks fundamental $live_symbol" ""
    add_case "live-json" "compare" "0" "$live_env" "-o json stocks compare $compare_args" ""
    add_case "live-json" "profile" "0" "$live_env" "-o json stocks profile $live_symbol" ""
    add_case "live-json" "financials" "0" "$live_env" "-o json stocks financials $live_symbol" ""
    add_case "live-json" "earnings" "0" "$live_env" "-o json stocks earnings $live_symbol" ""
    add_case "live-json" "sentiment" "0" "$live_env" "-o json stocks sentiment $live_symbol" ""
    add_case "live-json" "insights" "0" "$live_env" "-o json stocks insights $live_symbol" ""
    add_case "live-json" "news" "0" "$live_env" "-o json $news_args" ""
    add_case "live-json" "screen" "0" "$live_env" "-o json $screen_args" ""

    add_case "live-nonfinite" "valuation-bumi" "0" "$live_env" "-o json stocks valuation BUMI" "\"overall_signal\""
    add_case "live-nonfinite" "valuation-adro" "0" "$live_env" "-o json stocks valuation ADRO" "\"overall_signal\""
    add_case "live-nonfinite" "valuation-aims" "0" "$live_env" "-o json stocks valuation AIMS" "\"overall_signal\""
    add_case "live-nonfinite" "compare-bad-tickers" "0" "$live_env" "-o json stocks compare BUMI,ADRO,AIMS" "\"symbol\": \"BUMI.JK\""

    add_case "mock" "quote-table" "0" "$mock_env" "stocks quote $live_symbol" ""
    add_case "mock" "quote-json" "0" "$mock_env" "-o json stocks quote $live_symbol" ""
    add_case "mock" "history-table" "0" "$mock_env" "$history_args" ""
    add_case "mock" "history-json" "0" "$mock_env" "-o json $history_args" ""
    add_case "mock" "technical-table" "0" "$mock_env" "stocks technical $live_symbol" ""
    add_case "mock" "technical-json" "0" "$mock_env" "-o json stocks technical $live_symbol" ""
    add_case "mock" "growth-table" "0" "$mock_env" "stocks growth $live_symbol" ""
    add_case "mock" "growth-json" "0" "$mock_env" "-o json stocks growth $live_symbol" ""
    add_case "mock" "valuation-table" "0" "$mock_env" "stocks valuation $live_symbol" ""
    add_case "mock" "valuation-json" "0" "$mock_env" "-o json stocks valuation $live_symbol" ""
    add_case "mock" "risk-table" "0" "$mock_env" "stocks risk $live_symbol" ""
    add_case "mock" "risk-json" "0" "$mock_env" "-o json stocks risk $live_symbol" ""
    add_case "mock" "fundamental-table" "0" "$mock_env" "stocks fundamental $live_symbol" ""
    add_case "mock" "fundamental-json" "0" "$mock_env" "-o json stocks fundamental $live_symbol" ""
    add_case "mock" "compare-table" "0" "$mock_env" "stocks compare $compare_args" ""
    add_case "mock" "compare-json" "0" "$mock_env" "-o json stocks compare $compare_args" ""
    add_case "mock" "profile-table" "0" "$mock_env" "stocks profile $live_symbol" ""
    add_case "mock" "profile-json" "0" "$mock_env" "-o json stocks profile $live_symbol" ""
    add_case "mock" "financials-table" "0" "$mock_env" "stocks financials $live_symbol" ""
    add_case "mock" "financials-json" "0" "$mock_env" "-o json stocks financials $live_symbol" ""
    add_case "mock" "earnings-table" "0" "$mock_env" "stocks earnings $live_symbol" ""
    add_case "mock" "earnings-json" "0" "$mock_env" "-o json stocks earnings $live_symbol" ""
    add_case "mock" "sentiment-table" "0" "$mock_env" "stocks sentiment $live_symbol" ""
    add_case "mock" "sentiment-json" "0" "$mock_env" "-o json stocks sentiment $live_symbol" ""
    add_case "mock" "insights-table" "0" "$mock_env" "stocks insights $live_symbol" ""
    add_case "mock" "insights-json" "0" "$mock_env" "-o json stocks insights $live_symbol" ""
    add_case "mock" "news-table" "0" "$mock_env" "$news_args" ""
    add_case "mock" "news-json" "0" "$mock_env" "-o json $news_args" ""
    add_case "mock" "screen-table" "0" "$mock_env" "$screen_args" ""
    add_case "mock" "screen-json" "0" "$mock_env" "-o json $screen_args" ""

    add_case "cache" "quote-warm" "0" "$quote_zero_ttl_env" "stocks quote $live_symbol" ""
    add_case "cache" "quote-offline" "0" "$quote_zero_ttl_env" "--offline stocks quote $live_symbol" ""
    add_case "cache" "quote-stale-fallback" "0" "$quote_zero_ttl_env IDX_MOCK_ERROR=1" "stocks quote $live_symbol" "serving stale cache"
    add_case "cache" "technical-warm" "0" "$quote_zero_ttl_env" "stocks technical $live_symbol" ""
    add_case "cache" "technical-offline" "0" "$quote_zero_ttl_env" "--offline stocks technical $live_symbol" ""
    add_case "cache" "technical-stale-fallback" "0" "$quote_zero_ttl_env IDX_MOCK_ERROR=1" "stocks technical $live_symbol" "serving stale cache"
    add_case "cache" "profile-warm" "0" "$profile_zero_ttl_env" "stocks profile $live_symbol" ""
    add_case "cache" "profile-offline" "0" "$profile_zero_ttl_env" "--offline stocks profile $live_symbol" ""
    add_case "cache" "profile-stale-fallback" "0" "$profile_zero_ttl_env IDX_MOCK_ERROR=1" "stocks profile $live_symbol" "serving stale cache"

    add_case "routing" "yahoo-quote" "0" "$yahoo_mock_env" "stocks quote $live_symbol" ""
    add_case "routing" "auto-history-fallback" "0" "$mock_env" "$history_args" ""
    add_case "routing" "auto-technical-fallback" "0" "$mock_env" "stocks technical $live_symbol" ""
    add_case "routing" "explicit-msn-history" "0" "$mock_env" "stocks history $live_symbol --period 3mo --history-provider msn" "History for"

    add_case "errors" "invalid-provider-json" "1" "IDX_PROVIDER=bogus" "-o json version" "\"error\": true"
    add_case "errors" "profile-provider-gate-json" "1" "IDX_PROVIDER=yahoo" "-o json stocks profile $live_symbol" "requires --provider msn"
    add_case "errors" "invalid-screen-region-json" "1" "$mock_env" "-o json stocks screen --region eu" "invalid screener region"
    add_case "errors" "offline-no-cache" "1" "IDX_USE_MOCK_PROVIDER=1" "--offline --no-cache stocks quote $live_symbol" "cannot combine --offline with --no-cache"

    add_case "ownership" "releases-empty" "0" "" "ownership releases" "No ownership releases imported yet."
    add_case "ownership" "fetch-bing-unsupported" "1" "" "ownership import --fetch-bing $live_symbol" "--fetch-bing import is not implemented yet"

    add_case "ownership-import" "discover-default-above1" "0" "" "ownership discover --limit 1" "supported"
    add_case "ownership-import" "import-supported-above1" "0" "" "ownership import --url $OWNERSHIP_ABOVE1_URL" "Imported "
    add_case "ownership-import" "releases-after-import" "0" "" "ownership releases" "$OWNERSHIP_ABOVE1_URL"
    add_case "ownership-import" "import-legacy-above5" "1" "" "ownership import --url $OWNERSHIP_ABOVE5_URL" "legacy IDX \`above5\` ownership PDFs are not supported for import"
    add_case "ownership-import" "import-legacy-investor-type" "1" "" "ownership import --url $OWNERSHIP_INVESTOR_TYPE_URL" "legacy IDX \`investor-type\` ownership PDFs are not supported for import"
}

parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --mode)
                MODE="${2:-}"
                shift 2
                ;;
            --group)
                add_group_filters "${2:-}"
                shift 2
                ;;
            --symbol)
                SYMBOL="${2:-}"
                shift 2
                ;;
            --compare-symbols)
                COMPARE_SYMBOLS="${2:-}"
                shift 2
                ;;
            --news-limit)
                NEWS_LIMIT="${2:-}"
                shift 2
                ;;
            --screen-limit)
                SCREEN_LIMIT="${2:-}"
                shift 2
                ;;
            --workdir)
                WORKDIR="${2:-}"
                shift 2
                ;;
            --bin)
                BIN_PATH="${2:-}"
                BUILD=0
                shift 2
                ;;
            --no-build)
                BUILD=0
                shift
                ;;
            --dry-run)
                DRY_RUN=1
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

    case "$MODE" in
        live|mock|full) ;;
        *)
            echo "invalid --mode '$MODE' (expected live, mock, or full)" >&2
            exit 2
            ;;
    esac
}

prepare_paths() {
    if [[ -z "$WORKDIR" ]]; then
        WORKDIR="$ROOT_DIR/tmp/live-smoke/$(date +%Y%m%d-%H%M%S)"
    fi

    CONFIG_HOME="$WORKDIR/config"
    CACHE_HOME="$WORKDIR/cache"
    DATA_HOME="$WORKDIR/data"
    OWNERSHIP_DB="$WORKDIR/ownership/ownership.db"
    LOG_DIR="$WORKDIR/logs"

    BASE_ENV=(
        "XDG_CONFIG_HOME=$CONFIG_HOME"
        "XDG_CACHE_HOME=$CACHE_HOME"
        "XDG_DATA_HOME=$DATA_HOME"
        "IDX_OUTPUT=table"
        "IDX_NO_COLOR=1"
    )

    if (( DRY_RUN == 0 )); then
        mkdir -p "$CONFIG_HOME" "$CACHE_HOME" "$DATA_HOME" "$(dirname "$OWNERSHIP_DB")" "$LOG_DIR"
    fi
}

build_binary() {
    local build_log
    local stale_input

    if [[ -z "$BIN_PATH" ]]; then
        BIN_PATH="$ROOT_DIR/target/debug/idx"
    fi

    if (( DRY_RUN )); then
        return 0
    fi

    if (( BUILD )); then
        if ! command -v cargo >/dev/null 2>&1; then
            if [[ -x "$BIN_PATH" ]]; then
                printf 'info: cargo not found; reusing existing binary at %s\n' "$BIN_PATH"
                BUILD=0
            else
                echo "cargo not found; run inside nix develop or pass --bin/--no-build with an existing idx binary" >&2
                exit 1
            fi
        fi
    fi

    if (( BUILD == 0 )) && [[ "$BIN_PATH" == "$ROOT_DIR/target/debug/idx" && -x "$BIN_PATH" ]]; then
        stale_input="$(
            find \
                "$ROOT_DIR/src" \
                "$ROOT_DIR/tests" \
                "$ROOT_DIR/Cargo.toml" \
                "$ROOT_DIR/Cargo.lock" \
                "$ROOT_DIR/scripts/live-smoke.sh" \
                -type f \
                -newer "$BIN_PATH" \
                -print \
                -quit \
                2>/dev/null
        )"
        if [[ -n "$stale_input" ]]; then
            echo "refusing to run smoke checks against stale $BIN_PATH; newer input detected at $stale_input" >&2
            echo "rebuild first or omit --no-build so the runner refreshes target/debug/idx" >&2
            exit 1
        fi
    fi

    if (( BUILD )); then
        build_log="$LOG_DIR/build.log"
        (
            cd "$ROOT_DIR" || exit 1
            cargo build --quiet --bin idx
        ) >"$build_log" 2>&1
        if [[ $? -ne 0 ]]; then
            echo "build failed; see $build_log" >&2
            exit 1
        fi
    fi

    if [[ ! -x "$BIN_PATH" ]]; then
        echo "idx binary not found or not executable: $BIN_PATH" >&2
        exit 1
    fi
}

bootstrap_case() {
    local label="$1"
    shift
    local log_file="$LOG_DIR/bootstrap-${label}.log"

    if (( DRY_RUN )); then
        printf 'bootstrap %-18s %s\n' "$label" "$*"
        return 0
    fi

    (
        cd "$ROOT_DIR" || exit 1
        env "${BASE_ENV[@]}" "$BIN_PATH" "$@"
    ) >"$log_file" 2>&1

    if [[ $? -ne 0 ]]; then
        echo "bootstrap failed for $label; see $log_file" >&2
        exit 1
    fi
}

prepare_environment() {
    bootstrap_case "config-init" config init
    bootstrap_case "ownership-db" config set ownership.db_path "$OWNERSHIP_DB"
}

discover_ownership_import_url() {
    local family="$1"
    local log_file="$LOG_DIR/bootstrap-discover-${family}.log"
    local url

    if (( DRY_RUN )); then
        case "$family" in
            above1) printf 'https://example.invalid/above1-lamp1.pdf' ;;
            above5) printf 'https://example.invalid/above5-lamp1.pdf' ;;
            investor-type) printf 'https://example.invalid/investor-type-lamp1.pdf' ;;
            *) return 1 ;;
        esac
        return 0
    fi

    (
        cd "$ROOT_DIR" || exit 1
        env "${BASE_ENV[@]}" "$BIN_PATH" -o json ownership discover --family "$family" --limit 1
    ) >"$log_file" 2>&1

    if [[ $? -ne 0 ]]; then
        echo "bootstrap failed for ownership-discover-$family; see $log_file" >&2
        exit 1
    fi

    url="$(grep -m1 '"pdf_url"' "$log_file" | sed -E 's/.*"pdf_url": "([^"]+)".*/\1/')"
    if [[ -z "$url" ]]; then
        echo "bootstrap failed for ownership-discover-$family: could not parse pdf_url from $log_file" >&2
        exit 1
    fi

    printf '%s' "$url"
}

prepare_ownership_import_inputs() {
    if ! should_run_group "ownership-import"; then
        return 0
    fi

    OWNERSHIP_ABOVE1_URL="$(discover_ownership_import_url above1)"
    OWNERSHIP_ABOVE5_URL="$(discover_ownership_import_url above5)"
    OWNERSHIP_INVESTOR_TYPE_URL="$(discover_ownership_import_url investor-type)"
}

cache_case_starts_fresh() {
    local group="$1"
    local label="$2"

    [[ "$group" == "cache" && "$label" == *"-warm" ]]
}

reset_cache_for_case() {
    local label="$1"

    # Cache checks need a clean starting point so the warm/offline/stale trio
    # validates TTL-sensitive fallback behavior instead of reusing prior group state.
    bootstrap_case "cache-reset-${label}" cache clear
}

sanitize_name() {
    printf '%s' "$1" | tr -cs 'A-Za-z0-9._-' '_'
}

command_display() {
    local env_spec="$1"
    local cmd_spec="$2"

    printf 'env %s' "${BASE_ENV[*]}"
    if [[ -n "$env_spec" ]]; then
        printf ' %s' "$env_spec"
    fi
    printf ' %s %s' "$BIN_PATH" "$cmd_spec"
}

run_case_line() {
    local line="$1"
    local group label expected_exit env_spec cmd_spec expect_spec
    local safe_name log_file status
    local -a env_parts cmd_parts

    IFS="$SEP" read -r group label expected_exit env_spec cmd_spec expect_spec <<< "$line"

    if ! should_run_group "$group"; then
        return 0
    fi

    TOTAL_CASES=$((TOTAL_CASES + 1))
    safe_name="$(sanitize_name "${group}-${label}")"
    log_file="$(printf '%s/%03d_%s.log' "$LOG_DIR" "$TOTAL_CASES" "$safe_name")"

    printf '[%02d] %-10s %s\n' "$TOTAL_CASES" "$group" "$label"

    if cache_case_starts_fresh "$group" "$label"; then
        reset_cache_for_case "$label"
    fi

    if (( DRY_RUN )); then
        printf '     %s\n' "$(command_display "$env_spec" "$cmd_spec")"
        return 0
    fi

    env_parts=()
    cmd_parts=()

    if [[ -n "$env_spec" ]]; then
        read -r -a env_parts <<< "$env_spec"
    fi
    read -r -a cmd_parts <<< "$cmd_spec"

    (
        cd "$ROOT_DIR" || exit 1
        env "${BASE_ENV[@]}" "${env_parts[@]}" "$BIN_PATH" "${cmd_parts[@]}"
    ) >"$log_file" 2>&1
    status=$?

    if [[ "$status" -ne "$expected_exit" ]]; then
        FAILED_CASES=$((FAILED_CASES + 1))
        FAILURES+=("$group/$label (exit $status, expected $expected_exit) -> $log_file")
        printf '     FAIL exit=%s expected=%s log=%s\n' "$status" "$expected_exit" "$log_file"
        return 0
    fi

    if [[ -n "$expect_spec" ]] && ! grep -Fq -- "$expect_spec" "$log_file"; then
        FAILED_CASES=$((FAILED_CASES + 1))
        FAILURES+=("$group/$label (missing '$expect_spec') -> $log_file")
        printf '     FAIL missing=%s log=%s\n' "$expect_spec" "$log_file"
        return 0
    fi

    PASSED_CASES=$((PASSED_CASES + 1))
    printf '     PASS log=%s\n' "$log_file"
}

main() {
    local case_line

    parse_args "$@"
    prepare_paths

    printf 'mode: %s\n' "$MODE"
    printf 'workdir: %s\n' "$WORKDIR"
    printf 'symbol: %s\n' "$SYMBOL"
    printf 'compare symbols: %s\n' "$COMPARE_SYMBOLS"
    if [[ ${#GROUP_FILTERS[@]} -gt 0 ]]; then
        printf 'group filter: %s\n' "${GROUP_FILTERS[*]}"
    fi

    build_binary
    prepare_environment
    prepare_ownership_import_inputs
    register_cases

    if should_run_group "ownership-import"; then
        printf 'ownership above1 url: %s\n' "$OWNERSHIP_ABOVE1_URL"
        printf 'ownership above5 url: %s\n' "$OWNERSHIP_ABOVE5_URL"
        printf 'ownership investor-type url: %s\n' "$OWNERSHIP_INVESTOR_TYPE_URL"
    fi

    for case_line in "${CASES[@]}"; do
        run_case_line "$case_line"
    done

    if (( DRY_RUN )); then
        printf '\ndry-run complete: %d case(s) selected\n' "$TOTAL_CASES"
        exit 0
    fi

    printf '\nsummary: %d passed, %d failed, %d total\n' "$PASSED_CASES" "$FAILED_CASES" "$TOTAL_CASES"
    printf 'logs: %s\n' "$LOG_DIR"

    if (( FAILED_CASES > 0 )); then
        printf 'failures:\n'
        printf '  - %s\n' "${FAILURES[@]}"
        exit 1
    fi
}

main "$@"
