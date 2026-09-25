#!/usr/bin/env bash
# Offline regression tests for scripts/publish-ownership-snapshot.sh.
#
# Uses a fake `gh`, a fake `idx`, and a stub builder. No live network is touched.
# The test exercises the publication protocol (content-addressed SQLite uploaded
# and verified before the manifest commit point), the history-aware no-op logic,
# and the RESULT contract.
set -Eeuo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work="$(mktemp -d "${TMPDIR:-/tmp}/idx-publish-test.XXXXXX")"
state="$work/state"
bin_dir="$work/bin"
scripts_dir="$work/scripts"
out_dir="$work/out"
publisher="$scripts_dir/publish-ownership-snapshot.sh"

cleanup() {
    rm -rf "$work"
}
trap cleanup EXIT

failures=0
pass() { echo "ok   - $1"; }
fail() {
    echo "FAIL - $1" >&2
    failures=$((failures + 1))
}

mkdir -p "$bin_dir" "$scripts_dir" "$out_dir"
cp "$repo_root/scripts/publish-ownership-snapshot.sh" "$publisher"
chmod +x "$publisher"

# --- fake gh ---------------------------------------------------------------
cat >"$bin_dir/gh" <<'FAKE_GH'
#!/usr/bin/env bash
set -euo pipefail
state="${FAKE_GH_STATE:?}"
remote="$state/remote"
assets="$remote/assets"
mkdir -p "$assets"

if [[ -n "${FAKE_GH_FAIL_ALL:-}" ]]; then
    echo "fake gh: simulated network/auth failure" >&2
    exit 1
fi

file_sha() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

cmd="${1:-} ${2:-}"
case "$cmd" in
    "release download")
        shift 2
        pattern=""
        output=""
        while [[ $# -gt 0 ]]; do
            case "$1" in
                --pattern) pattern="$2"; shift 2 ;;
                --output) output="$2"; shift 2 ;;
                --repo) shift 2 ;;
                --clobber) shift ;;
                *) shift ;;
            esac
        done
        [[ -n "$pattern" ]] || pattern="ownership-snapshot-manifest.json"
        if [[ ! -f "$assets/$pattern" ]]; then
            exit 1
        fi
        if [[ "$output" == "-" || -z "$output" ]]; then
            cat "$assets/$pattern"
        else
            cp "$assets/$pattern" "$output"
        fi
        ;;
    "release view")
        if [[ ! -f "$remote/.release-exists" ]]; then
            exit 1
        fi
        if [[ "$*" == *"--json assets"* ]]; then
            printf '{"assets":['
            first=1
            for f in "$assets"/*; do
                [[ -e "$f" ]] || continue
                name="$(basename "$f")"
                size="$(wc -c <"$f" | tr -d ' ')"
                if [[ "$first" == "1" ]]; then first=0; else printf ','; fi
                if [[ -n "${FAKE_GH_EMIT_DIGEST:-}" ]]; then
                    printf '{"name":"%s","size":%s,"digest":"sha256:%s"}' "$name" "$size" "$(file_sha "$f")"
                else
                    printf '{"name":"%s","size":%s}' "$name" "$size"
                fi
            done
            printf ']}'
        else
            exit 0
        fi
        ;;
    "release create")
        touch "$remote/.release-exists"
        ;;
    "release upload")
        shift 3
        files=()
        while [[ $# -gt 0 ]]; do
            case "$1" in
                --repo) shift 2 ;;
                --clobber) shift ;;
                *) files+=("$1"); shift ;;
            esac
        done
        for f in "${files[@]}"; do
            name="$(basename "$f")"
            if [[ -n "${FAKE_GH_FAIL_UPLOAD_ALWAYS:-}" && "$name" == *"$FAKE_GH_FAIL_UPLOAD_ALWAYS"* ]]; then
                echo "fake gh: upload refused for $name" >&2
                exit 1
            fi
            if [[ -n "${FAKE_GH_FAIL_UPLOAD_ONCE:-}" && "$name" == *"$FAKE_GH_FAIL_UPLOAD_ONCE"* && ! -f "$state/fail-once-used" ]]; then
                touch "$state/fail-once-used"
                echo "fake gh: simulated one-time upload failure for $name" >&2
                exit 1
            fi
            mkdir -p "$assets"
            cp "$f" "$assets/$name"
            if [[ -n "${FAKE_GH_CORRUPT_UPLOAD_MATCH:-}" && "$name" == *"$FAKE_GH_CORRUPT_UPLOAD_MATCH"* ]]; then
                # Flip the first byte but keep the length, to model silent upload corruption.
                first_byte="$(head -c 1 "$assets/$name")"
                if [[ "$first_byte" == "Z" ]]; then repl="Y"; else repl="Z"; fi
                printf '%s' "$repl" | dd of="$assets/$name" bs=1 count=1 conv=notrunc status=none
            fi
        done
        ;;
    *)
        echo "fake gh: unexpected command: $*" >&2
        exit 99
        ;;
esac
FAKE_GH
chmod +x "$bin_dir/gh"

# --- fake idx --------------------------------------------------------------
cat >"$bin_dir/idx" <<'FAKE_IDX'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == "version" ]]; then
    exit 0
fi
if [[ "$*" == *"ownership discover"* ]]; then
    printf '%s\n' "${FAKE_DISCOVER_JSON:?FAKE_DISCOVER_JSON not set}"
    exit 0
fi
echo "fake idx: unexpected args: $*" >&2
exit 99
FAKE_IDX
chmod +x "$bin_dir/idx"

# --- stub builder ----------------------------------------------------------
cat >"$scripts_dir/build-latest-ownership-snapshot.sh" <<'STUB_BUILDER'
#!/usr/bin/env bash
set -euo pipefail
out=""
history="0"
repo="example/repo"
tag="ownership-snapshot-current"
while [[ $# -gt 0 ]]; do
    case "$1" in
        --output-dir) out="$2"; shift 2 ;;
        --history) history="$2"; shift 2 ;;
        --repo) repo="$2"; shift 2 ;;
        --release-tag) tag="$2"; shift 2 ;;
        --idx-bin) shift 2 ;;
        --keep-workdir) shift ;;
        *) shift ;;
    esac
done
[[ -n "$out" ]] || { echo "stub builder: missing --output-dir" >&2; exit 2; }
printf 'history=%s\n' "$history" >>"${FAKE_BUILDER_CALLS:?}"

as_of="${FAKE_BUILD_AS_OF:-2026-08-31}"
if [[ -n "${FAKE_BUILD_RELEASE_COUNT:-}" ]]; then
    count="$FAKE_BUILD_RELEASE_COUNT"
else
    avail="${FAKE_AVAILABLE_HISTORY:-$history}"
    if (( history < avail )); then
        count=$(( 1 + history ))
    else
        count=$(( 1 + avail ))
    fi
fi
variant="${FAKE_BUILD_VARIANT:-}"

tmp="$out/.stub-sqlite.tmp"
printf 'stub-sqlite as_of=%s count=%s variant=%s' "$as_of" "$count" "$variant" >"$tmp"
if command -v sha256sum >/dev/null 2>&1; then
    sha="$(sha256sum "$tmp" | awk '{print $1}')"
else
    sha="$(shasum -a 256 "$tmp" | awk '{print $1}')"
fi
size="$(wc -c <"$tmp" | tr -d ' ')"
name="ownership-snapshot-${as_of}-${sha}.sqlite"
mv "$tmp" "$out/$name"
url="https://github.com/${repo}/releases/download/${tag}/${name}"
cat >"$out/ownership-snapshot-manifest.json" <<JSON
{"schema_version":1,"generated_at":"2026-08-31T00:00:00Z","snapshot":{"kind":"sqlite","compression":"none","version":"$as_of","download_url":"$url","sqlite_sha256":"$sha","size_bytes":$size,"release_count":$count,"latest_as_of_date":"$as_of","latest_release_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","latest_row_count":10,"ticker_count":5}}
JSON
STUB_BUILDER
chmod +x "$scripts_dir/build-latest-ownership-snapshot.sh"

# --- harness helpers -------------------------------------------------------
export TMPDIR="$work/tmp"
mkdir -p "$TMPDIR"
export FAKE_GH_STATE="$state"
export FAKE_BUILDER_CALLS="$state/builder-calls"
export FAKE_DISCOVER_JSON='[{"status":"supported","format":"xlsx","as_of_date":"2026-08-31"}]'
export FAKE_BUILD_AS_OF="2026-08-31"
export FAKE_AVAILABLE_HISTORY="0"
unset FAKE_BUILD_RELEASE_COUNT FAKE_BUILD_VARIANT FAKE_GH_FAIL_ONCE \
    FAKE_GH_FAIL_UPLOAD_ONCE FAKE_GH_FAIL_UPLOAD_ALWAYS FAKE_GH_FAIL_ALL \
    FAKE_GH_EMIT_DIGEST FAKE_GH_CORRUPT_UPLOAD_MATCH 2>/dev/null || true

PUBLISH_OUT=""
PUBLISH_STATUS=""
PUBLISH_LAST=""

run_publisher() {
    local out status
    set +e
    out="$(PATH="$bin_dir:$PATH" "$publisher" \
        --idx-bin "$bin_dir/idx" \
        --output-dir "$out_dir" \
        --repo example/repo \
        --release-tag ownership-snapshot-current "$@" 2>&1)"
    status=$?
    set -e
    PUBLISH_OUT="$out"
    PUBLISH_STATUS="$status"
    PUBLISH_LAST="$(printf '%s\n' "$out" | tail -n 1)"
}

reset_remote() {
    rm -rf "$state/remote" "$state/builder-calls" "$state/fail-once-used"
    mkdir -p "$state/remote/assets"
    rm -rf "$out_dir"
    mkdir -p "$out_dir"
}

builder_calls() {
    if [[ -f "$state/builder-calls" ]]; then
        wc -l <"$state/builder-calls" | tr -d ' '
    else
        echo 0
    fi
}

asset_count() {
    find "$state/remote/assets" -type f -name '*.sqlite' 2>/dev/null | wc -l | tr -d ' '
}

remote_manifest_field() {
    jq -r "$1" "$state/remote/assets/ownership-snapshot-manifest.json" 2>/dev/null || true
}

remote_asset_exists() {
    [[ -f "$state/remote/assets/$1" ]]
}

# Flip the first byte of a remote asset but keep its length.
corrupt_remote_asset_same_length() {
    local name="$1"
    local path="$state/remote/assets/$name"
    local first_byte repl
    first_byte="$(head -c 1 "$path")"
    if [[ "$first_byte" == "Z" ]]; then repl="Y"; else repl="Z"; fi
    printf '%s' "$repl" | dd of="$path" bs=1 count=1 conv=notrunc status=none
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

publish_ok() {
    local label="$1" expected="$2"
    if [[ "$PUBLISH_STATUS" == "0" && "$PUBLISH_LAST" == "$expected" ]]; then
        pass "$label"
    else
        fail "$label (status=$PUBLISH_STATUS last='$PUBLISH_LAST')"
        printf '%s\n' "$PUBLISH_OUT" >&2
    fi
}

expect_contains() {
    local label="$1" haystack="$2" needle="$3"
    if [[ "$haystack" == *"$needle"* ]]; then
        pass "$label"
    else
        fail "$label (missing '$needle')"
        printf '%s\n' "$haystack" >&2
    fi
}

# ===========================================================================
# 1. successful publish, then a cheap no-op on the next run
# ===========================================================================
reset_remote
run_publisher
publish_ok "publishes the first snapshot" "RESULT: published 2026-08-31"
if [[ "$(builder_calls)" == "1" ]]; then
    pass "first publish builds once"
else
    fail "first publish builds once (calls=$(builder_calls))"
fi
if remote_asset_exists "$(remote_manifest_field '.snapshot.download_url' | sed 's#.*/##')"; then
    pass "published manifest points at a present asset"
else
    fail "published manifest points at a present asset"
fi

run_publisher
publish_ok "second run is up-to-date without rebuilding" "RESULT: up-to-date 2026-08-31"
if [[ "$(builder_calls)" == "1" ]]; then
    pass "no-op run skips the builder"
else
    fail "no-op run skips the builder (calls=$(builder_calls))"
fi

# ===========================================================================
# 2. partial SQLite upload fails, then a retry repairs the publication
# ===========================================================================
reset_remote
export FAKE_GH_FAIL_UPLOAD_ONCE=".sqlite"
run_publisher
if [[ "$PUBLISH_STATUS" != "0" ]] && [[ "$PUBLISH_LAST" == "RESULT: FAILED stage=upload"* ]]; then
    pass "partial SQLite upload failure is reported and exits non-zero"
else
    fail "partial SQLite upload failure is reported (status=$PUBLISH_STATUS last='$PUBLISH_LAST')"
fi
if ! remote_asset_exists "ownership-snapshot-manifest.json"; then
    pass "manifest is not published while the SQLite asset is missing"
else
    fail "manifest is not published while the SQLite asset is missing"
fi

unset FAKE_GH_FAIL_UPLOAD_ONCE
run_publisher
publish_ok "retry publishes after a partial upload" "RESULT: published 2026-08-31"

# ===========================================================================
# 3. manifest upload fails after SQLite succeeds, then a retry repairs it
# ===========================================================================
reset_remote
export FAKE_GH_FAIL_UPLOAD_ONCE="ownership-snapshot-manifest.json"
run_publisher
if [[ "$PUBLISH_STATUS" != "0" ]] && [[ "$PUBLISH_LAST" == "RESULT: FAILED stage=publish"* ]]; then
    pass "manifest upload failure is reported and exits non-zero"
else
    fail "manifest upload failure is reported (status=$PUBLISH_STATUS last='$PUBLISH_LAST')"
fi
if [[ "$(asset_count)" == "1" ]]; then
    pass "SQLite artifact from the failed manifest publish is retained"
else
    fail "SQLite artifact from the failed manifest publish is retained (count=$(asset_count))"
fi

unset FAKE_GH_FAIL_UPLOAD_ONCE
run_publisher
publish_ok "retry publishes the manifest after a manifest failure" "RESULT: published 2026-08-31"

# ===========================================================================
# 4. forced same-date refresh preserves the previous artifact
# ===========================================================================
reset_remote
run_publisher
old_asset="$(remote_manifest_field '.snapshot.download_url' | sed 's#.*/##')"
export FAKE_BUILD_VARIANT="replacement"
run_publisher --force
publish_ok "forced same-date refresh publishes" "RESULT: published 2026-08-31"
unset FAKE_BUILD_VARIANT
new_asset="$(remote_manifest_field '.snapshot.download_url' | sed 's#.*/##')"
if [[ "$old_asset" != "$new_asset" ]] && remote_asset_exists "$old_asset" && remote_asset_exists "$new_asset"; then
    pass "forced refresh keeps the previous content-addressed artifact"
else
    fail "forced refresh keeps the previous content-addressed artifact (old=$old_asset new=$new_asset count=$(asset_count))"
fi

# ===========================================================================
# 5. a trustworthy-looking manifest with a missing asset forces a republish
# ===========================================================================
reset_remote
mkdir -p "$state/remote/assets"
touch "$state/remote/.release-exists"
cat >"$state/remote/assets/ownership-snapshot-manifest.json" <<'JSON'
{"schema_version":1,"generated_at":"2026-08-31T00:00:00Z","snapshot":{"kind":"sqlite","compression":"none","version":"2026-08-31","download_url":"https://github.com/example/repo/releases/download/ownership-snapshot-current/ownership-snapshot-2026-08-31-missing.sqlite","sqlite_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size_bytes":999,"release_count":1,"latest_as_of_date":"2026-08-31","latest_release_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","latest_row_count":10,"ticker_count":5}}
JSON
run_publisher
publish_ok "missing referenced asset forces a republish" "RESULT: published 2026-08-31"
if remote_asset_exists "$(remote_manifest_field '.snapshot.download_url' | sed 's#.*/##')"; then
    pass "republished manifest points at a present asset"
else
    fail "republished manifest points at a present asset"
fi

# ===========================================================================
# 5b. same-size corruption of a legacy (digest-less) asset forces republish
# ===========================================================================
reset_remote
run_publisher
published_asset="$(remote_manifest_field '.snapshot.download_url' | sed 's#.*/##')"
corrupt_remote_asset_same_length "$published_asset"
run_publisher
publish_ok "same-size corruption of a legacy asset forces republish" "RESULT: published 2026-08-31"
republished_asset="$(remote_manifest_field '.snapshot.download_url' | sed 's#.*/##')"
if [[ "$(sha256_of "$state/remote/assets/$republished_asset")" == "$(remote_manifest_field '.snapshot.sqlite_sha256')" ]]; then
    pass "republished legacy asset matches the manifest sha256"
else
    fail "republished legacy asset matches the manifest sha256"
fi

# ===========================================================================
# 5c. sha256 digest metadata detects same-size corruption
# ===========================================================================
reset_remote
export FAKE_GH_EMIT_DIGEST="1"
run_publisher
run_publisher
publish_ok "intact digest-verified asset no-ops" "RESULT: up-to-date 2026-08-31"
published_asset="$(remote_manifest_field '.snapshot.download_url' | sed 's#.*/##')"
corrupt_remote_asset_same_length "$published_asset"
run_publisher
publish_ok "same-size corruption with digest metadata forces republish" "RESULT: published 2026-08-31"
unset FAKE_GH_EMIT_DIGEST

# ===========================================================================
# 5d. upload corruption cannot commit the manifest (digest and digest-less)
# ===========================================================================
reset_remote
export FAKE_GH_CORRUPT_UPLOAD_MATCH=".sqlite"
export FAKE_GH_EMIT_DIGEST="1"
run_publisher
if [[ "$PUBLISH_STATUS" != "0" ]] && [[ "$PUBLISH_LAST" == "RESULT: FAILED stage=upload"* ]]; then
    pass "digest-verified upload corruption is rejected"
else
    fail "digest-verified upload corruption is rejected (status=$PUBLISH_STATUS last='$PUBLISH_LAST')"
fi
if ! remote_asset_exists "ownership-snapshot-manifest.json"; then
    pass "corrupt upload does not commit a manifest (digest mode)"
else
    fail "corrupt upload does not commit a manifest (digest mode)"
fi
unset FAKE_GH_EMIT_DIGEST
reset_remote
export FAKE_GH_CORRUPT_UPLOAD_MATCH=".sqlite"
run_publisher
if [[ "$PUBLISH_STATUS" != "0" ]] && [[ "$PUBLISH_LAST" == "RESULT: FAILED stage=upload"* ]]; then
    pass "digest-less upload corruption is rejected after fetch and hash"
else
    fail "digest-less upload corruption is rejected (status=$PUBLISH_STATUS last='$PUBLISH_LAST')"
fi
if ! remote_asset_exists "ownership-snapshot-manifest.json"; then
    pass "corrupt upload does not commit a manifest (digest-less mode)"
else
    fail "corrupt upload does not commit a manifest (digest-less mode)"
fi
unset FAKE_GH_CORRUPT_UPLOAD_MATCH

# ===========================================================================
# 5e. a manifest whose URL names a different repo/tag is not trusted
# ===========================================================================
reset_remote
run_publisher
asset_name="$(remote_manifest_field '.snapshot.download_url' | sed 's#.*/##')"
manifest_path="$state/remote/assets/ownership-snapshot-manifest.json"
jq --arg url "https://github.com/other/repo/releases/download/other-tag/$asset_name" \
    '.snapshot.download_url = $url' "$manifest_path" >"$manifest_path.tmp"
mv "$manifest_path.tmp" "$manifest_path"
run_publisher
publish_ok "manifest URL for a different repo/tag is not trusted" "RESULT: published 2026-08-31"

# ===========================================================================
# 6. legacy PDF discovery ends in a post-build no-op
# ===========================================================================
reset_remote
run_publisher
calls_before="$(builder_calls)"
export FAKE_DISCOVER_JSON='[{"status":"supported","format":"pdf","as_of_date":null,"pdf_url":"https://example.invalid/legacy.pdf"}]'
export FAKE_BUILD_RELEASE_COUNT="1"
run_publisher
publish_ok "legacy PDF source still no-ops after build" "RESULT: up-to-date 2026-08-31"
if [[ "$(builder_calls)" == "$((calls_before + 1))" ]]; then
    pass "legacy PDF source runs the builder once"
else
    fail "legacy PDF source runs the builder once (before=$calls_before after=$(builder_calls))"
fi
unset FAKE_BUILD_RELEASE_COUNT
export FAKE_DISCOVER_JSON='[{"status":"supported","format":"xlsx","as_of_date":"2026-08-31"}]'

# ===========================================================================
# 7. a larger --history upgrades an existing same-date snapshot
# ===========================================================================
reset_remote
run_publisher
export FAKE_AVAILABLE_HISTORY="6"
export FAKE_DISCOVER_JSON='[
  {"status":"supported","format":"xlsx","as_of_date":"2026-08-31"},
  {"status":"supported","format":"xlsx","as_of_date":"2026-07-31"},
  {"status":"supported","format":"xlsx","as_of_date":"2026-06-30"},
  {"status":"supported","format":"xlsx","as_of_date":"2026-05-31"},
  {"status":"supported","format":"xlsx","as_of_date":"2026-04-30"},
  {"status":"supported","format":"xlsx","as_of_date":"2026-03-31"},
  {"status":"supported","format":"xlsx","as_of_date":"2026-02-28"}
]'
run_publisher --history 5
publish_ok "history increase republishes the same date" "RESULT: published 2026-08-31"
if [[ "$(remote_manifest_field '.snapshot.release_count')" == "6" ]]; then
    pass "history increase publishes the requested coverage"
else
    fail "history increase publishes the requested coverage (rc=$(remote_manifest_field '.snapshot.release_count'))"
fi
export FAKE_AVAILABLE_HISTORY="0"
export FAKE_DISCOVER_JSON='[{"status":"supported","format":"xlsx","as_of_date":"2026-08-31"}]'

# ===========================================================================
# 8. limited available history does not rebuild forever
# ===========================================================================
reset_remote
export FAKE_AVAILABLE_HISTORY="2"
export FAKE_DISCOVER_JSON='[
  {"status":"supported","format":"xlsx","as_of_date":"2026-08-31"},
  {"status":"supported","format":"xlsx","as_of_date":"2026-07-31"},
  {"status":"supported","format":"xlsx","as_of_date":"2026-06-30"}
]'
run_publisher --history 2
publish_ok "limited-history baseline publishes" "RESULT: published 2026-08-31"
calls_before="$(builder_calls)"
run_publisher --history 5
publish_ok "requesting more than available history no-ops" "RESULT: up-to-date 2026-08-31"
if [[ "$(builder_calls)" == "$calls_before" ]]; then
    pass "limited available history does not rebuild repeatedly"
else
    fail "limited available history does not rebuild repeatedly (before=$calls_before after=$(builder_calls))"
fi
export FAKE_AVAILABLE_HISTORY="0"
export FAKE_DISCOVER_JSON='[{"status":"supported","format":"xlsx","as_of_date":"2026-08-31"}]'

# ===========================================================================
# 9. invalid arguments fail before an early exit
# ===========================================================================
reset_remote
run_publisher
calls_before="$(builder_calls)"
run_publisher --history abc
if [[ "$PUBLISH_STATUS" == "2" ]] && [[ "$PUBLISH_LAST" == "RESULT: FAILED stage=arguments (exit 2)" ]]; then
    pass "invalid --history fails even when the snapshot is current"
else
    fail "invalid --history fails on early exit (status=$PUBLISH_STATUS last='$PUBLISH_LAST')"
fi
if [[ "$(builder_calls)" == "$calls_before" ]]; then
    pass "invalid arguments do not run the builder"
else
    fail "invalid arguments do not run the builder"
fi

run_publisher --history 1001
if [[ "$PUBLISH_STATUS" == "2" ]] && [[ "$PUBLISH_LAST" == "RESULT: FAILED stage=arguments (exit 2)" ]]; then
    pass "over-max --history is rejected"
else
    fail "over-max --history is rejected (status=$PUBLISH_STATUS last='$PUBLISH_LAST')"
fi

run_publisher --history 999999999999999999999999999999
if [[ "$PUBLISH_STATUS" == "2" ]] && [[ "$PUBLISH_LAST" == "RESULT: FAILED stage=arguments (exit 2)" ]]; then
    pass "enormous --history is rejected without wrapping negative"
else
    fail "enormous --history is rejected (status=$PUBLISH_STATUS last='$PUBLISH_LAST')"
fi

run_publisher --history 1000
publish_ok "boundary --history 1000 is accepted" "RESULT: up-to-date 2026-08-31"

# ===========================================================================
# 9b. --keep-workdir keeps RESULT as the final output line
# ===========================================================================
reset_remote
run_publisher --keep-workdir
if [[ "$PUBLISH_LAST" == "RESULT: published 2026-08-31" ]]; then
    pass "--keep-workdir keeps RESULT as the final line"
else
    fail "--keep-workdir keeps RESULT as the final line (last='$PUBLISH_LAST')"
fi
expect_contains "--keep-workdir announces the retained path" "$PUBLISH_OUT" "Keeping publish workdir:"
after_result="$(printf '%s\n' "$PUBLISH_OUT" | awk '/^RESULT:/{seen=1; next} seen{print}')"
if [[ -z "$after_result" ]]; then
    pass "--keep-workdir prints nothing after RESULT"
else
    fail "--keep-workdir prints nothing after RESULT (trailing='$after_result')"
fi

# ===========================================================================
# 10. auth/network errors never become a trustworthy up-to-date
# ===========================================================================
reset_remote
export FAKE_GH_FAIL_ALL="1"
run_publisher
unset FAKE_GH_FAIL_ALL
if [[ "$PUBLISH_LAST" != *"up-to-date"* ]] && [[ "$PUBLISH_STATUS" != "0" ]]; then
    pass "gh failure is reported, not converted into up-to-date"
else
    fail "gh failure is reported, not converted into up-to-date (status=$PUBLISH_STATUS last='$PUBLISH_LAST')"
fi

# ===========================================================================
# 11. build-ownership-snapshot.sh emits a content-addressed, self-consistent pair
# ===========================================================================
if command -v sqlite3 >/dev/null 2>&1; then
    content_dir="$work/content-addressed"
    mkdir -p "$content_dir"
    db="$content_dir/source.db"
    sqlite3 "$db" <<'SQL'
CREATE TABLE ownership_releases (as_of_date TEXT, sha256 TEXT, row_count INTEGER, imported_at INTEGER);
CREATE TABLE tickers (code TEXT);
INSERT INTO ownership_releases VALUES ('2026-08-31', 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc', 10, 1);
INSERT INTO tickers VALUES ('BBCA');
SQL
    set +e
    "$repo_root/scripts/build-ownership-snapshot.sh" \
        --db "$db" \
        --output-dir "$content_dir/out" \
        --base-url "https://example.invalid/base" >"$content_dir/build.log" 2>&1
    build_status=$?
    set -e
    artifacts=("$content_dir/out"/ownership-snapshot-*.sqlite)
    if [[ "$build_status" == "0" && "${#artifacts[@]}" == "1" ]]; then
        artifact_name="$(basename "${artifacts[0]}")"
        artifact_sha="$(jq -r '.snapshot.sqlite_sha256' "$content_dir/out/ownership-snapshot-manifest.json")"
        artifact_url="$(jq -r '.snapshot.download_url' "$content_dir/out/ownership-snapshot-manifest.json")"
        if [[ "$artifact_name" =~ ^ownership-snapshot-2026-08-31-[0-9a-f]{64}\.sqlite$ ]] \
            && [[ "$artifact_name" == *"$artifact_sha"* ]] \
            && [[ "$artifact_url" == "https://example.invalid/base/$artifact_name" ]]; then
            pass "builder emits a content-addressed, self-consistent artifact pair"
        else
            fail "builder emits a content-addressed pair (name=$artifact_name sha=$artifact_sha url=$artifact_url)"
        fi
    else
        fail "builder emits one artifact (status=$build_status count=${#artifacts[@]})"
        cat "$content_dir/build.log" >&2
    fi
else
    echo "skip - sqlite3 not found; content-addressed builder check skipped" >&2
fi

# A matching asset basename must not authorize a broken nested download URL.
reset_remote
run_publisher
remote_manifest="$state/remote/assets/ownership-snapshot-manifest.json"
jq '.snapshot.download_url |= sub("/ownership-snapshot-current/"; "/ownership-snapshot-current/not-an-asset/")' \
    "$remote_manifest" > "$state/nested-manifest.json"
cp "$state/nested-manifest.json" "$remote_manifest"
run_publisher
publish_ok "nested download URL cannot authorize a no-op" "RESULT: published 2026-08-31"

if ((failures > 0)); then
    echo "publish-ownership-snapshot tests: ${failures} failure(s)" >&2
    exit 1
fi
echo "publish-ownership-snapshot tests: all passed"
