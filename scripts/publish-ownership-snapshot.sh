#!/usr/bin/env bash

set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/publish-ownership-snapshot.sh --output-dir <dir> [options]

Build and publish the latest supported IDX/KSEI ownership snapshot to the stable
GitHub release used by `idx ownership sync`.

Safe to run daily: unless --force is given, it skips the upload when the
published snapshot already has the latest as-of date and at least the requested
historical coverage. The published manifest is trusted only when the SQLite
artifact it references is at the configured repo/tag and matches the manifest
SHA-256 and size.

The immutable, content-addressed SQLite artifact is uploaded and verified
before the manifest is published, and the manifest is the final commit point.
A partial upload therefore never leaves consumers following a manifest whose
artifact is missing, and a retry repairs the publication.

The last line is always one of:
  RESULT: published <as-of>
  RESULT: up-to-date <as-of>
  RESULT: FAILED stage=<stage> (exit <code>)

Options:
  --idx-bin <path>       idx binary to use (default: ./target/debug/idx)
  --output-dir <dir>     Directory to write the copied snapshot and manifest
  --repo <owner/name>    GitHub repo used for release upload and public URLs
                         (default: 0xrsydn/idx-cli)
  --release-tag <tag>    Stable GitHub release tag used for snapshot assets
                         (default: ownership-snapshot-current)
  --history <n>          Request at least <n> previous monthly above-1% reports
                         in addition to the latest (default: 0). Fewer reports
                         are accepted when the source only has fewer months.
  --max-snapshot-bytes <n>
                         Passed to the builder: refuse snapshots above this
                         size (default: 10485760, the limit of idx <= v0.2.3)
  --force                Upload even if the published snapshot is up to date
  --build                Run `cargo build` before publishing
  --keep-workdir         Keep the temp workdir created by the builder helper
  --help                 Show this help
EOF
}

IDX_BIN="./target/debug/idx"
OUTPUT_DIR=""
REPO_FULL_NAME="0xrsydn/idx-cli"
RELEASE_TAG="ownership-snapshot-current"
BUILD_FIRST="0"
KEEP_WORKDIR="0"
HISTORY="0"
MAX_SNAPSHOT_BYTES=""
FORCE="0"
PUBLISH_WORKDIR=""
STAGE="arguments"

# Explicit failures: print the reason, then the RESULT line, then exit.
fail() {
    local status="$1"
    shift
    printf '%s\n' "$*" >&2
    printf 'RESULT: FAILED stage=%s (exit %s)\n' "$STAGE" "$status" >&2
    exit "$status"
}

# Unexpected command failures (set -e). errtrace is deliberately off: with
# it, a failure inside $(...) would run this in the subshell and the parent.
on_error() {
    local status=$?
    printf 'RESULT: FAILED stage=%s (exit %s)\n' "$STAGE" "$status" >&2
    exit "$status"
}

cleanup() {
    if [[ -z "$PUBLISH_WORKDIR" ]]; then
        return
    fi

    # Keep the RESULT line as the final line: never print here. The retained
    # path is announced when the workdir is created.
    if [[ "$KEEP_WORKDIR" == "1" ]]; then
        return
    fi

    rm -rf "$PUBLISH_WORKDIR"
}

trap cleanup EXIT
trap on_error ERR

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
        --force)
            FORCE="1"
            shift
            ;;
        --build)
            BUILD_FIRST="1"
            shift
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
            usage >&2
            fail 2 "unknown argument: $1"
            ;;
    esac
done

if [[ -z "$OUTPUT_DIR" ]]; then
    usage >&2
    fail 2 "--output-dir is required"
fi

# Validate arguments before any early exit so bad input always fails.
if ! [[ "$HISTORY" =~ ^[0-9]+$ ]]; then
    usage >&2
    fail 2 "--history must be a non-negative integer"
fi
# Strip leading zeros, then bound the value before arithmetic. A raw value
# with thousands of digits would wrap negative in $((10#$HISTORY)) and could
# make a history request look already satisfied.
HISTORY_DIGITS="${HISTORY#"${HISTORY%%[!0]*}"}"
MAX_HISTORY=1000
if [[ -z "$HISTORY_DIGITS" ]]; then
    HISTORY=0
else
    if (( ${#HISTORY_DIGITS} > 6 )); then
        usage >&2
        fail 2 "--history must be at most $MAX_HISTORY"
    fi
    HISTORY=$((10#$HISTORY_DIGITS))
    if (( HISTORY > MAX_HISTORY )); then
        usage >&2
        fail 2 "--history must be at most $MAX_HISTORY"
    fi
fi

if [[ -z "$IDX_BIN" ]]; then
    usage >&2
    fail 2 "--idx-bin must not be empty"
fi
if [[ -z "$REPO_FULL_NAME" ]]; then
    usage >&2
    fail 2 "--repo must not be empty"
fi
if [[ -z "$RELEASE_TAG" ]]; then
    usage >&2
    fail 2 "--release-tag must not be empty"
fi

if ! command -v gh >/dev/null 2>&1; then
    fail 1 "gh is required for GitHub release upload"
fi

if ! command -v jq >/dev/null 2>&1; then
    fail 1 "jq is required for manifest and release metadata processing"
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILDER="$SCRIPT_DIR/build-latest-ownership-snapshot.sh"

if [[ ! -x "$BUILDER" ]]; then
    fail 1 "required helper script is missing or not executable: $BUILDER"
fi

mkdir -p "$OUTPUT_DIR"

PUBLISH_WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/idx-ownership-publish.XXXXXX")"
if [[ "$KEEP_WORKDIR" == "1" ]]; then
    printf 'Keeping publish workdir: %s\n' "$PUBLISH_WORKDIR"
fi

# Published manifest, or empty when none exists or it cannot be read.
published_manifest() {
    gh release download "$RELEASE_TAG" \
        --repo "$REPO_FULL_NAME" \
        --pattern ownership-snapshot-manifest.json \
        --output - 2>/dev/null || true
}

# Release assets as JSON. Fails when the release or the GitHub API is not
# readable; callers must treat that as "not up to date", never as current.
remote_assets_json() {
    gh release view "$RELEASE_TAG" \
        --repo "$REPO_FULL_NAME" \
        --json assets 2>/dev/null
}

# Size of one release asset in bytes, or empty when it is not present.
remote_asset_size() {
    local assets_json="$1"
    local asset_name="$2"
    jq -r --arg name "$asset_name" \
        '.assets[]? | select(.name == $name) | .size' \
        <<< "$assets_json" 2>/dev/null || true
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
    fail 1 "sha256sum/shasum not found in PATH"
}

# Require the exact asset URL. A matching basename under an extra path is
# not a valid GitHub release download URL.
published_url_matches() {
    local url="$1"
    local asset_name="${url##*/}"
    [[ "$asset_name" =~ ^[A-Za-z0-9._-]+\.sqlite$ ]] || return 1
    [[ "$url" == "https://github.com/${REPO_FULL_NAME}/releases/download/${RELEASE_TAG}/${asset_name}" ]]
}

# Full SHA-256 of a release asset. Prefer GitHub's sha256 digest metadata and
# fall back to downloading the asset for legacy releases that predate it.
# Prints the lowercase hex digest, or returns non-zero when unavailable.
remote_asset_sha256() {
    local assets_json="$1"
    local asset_name="$2"
    local digest hex tmp result

    digest="$(jq -r --arg name "$asset_name" \
        '.assets[]? | select(.name == $name) | (.digest // empty)' \
        <<< "$assets_json" 2>/dev/null || true)"
    if [[ "$digest" == sha256:* ]]; then
        hex="${digest#sha256:}"
        hex="$(printf '%s' "$hex" | tr 'A-F' 'a-f')"
        if [[ "$hex" =~ ^[0-9a-f]{64}$ ]]; then
            printf '%s' "$hex"
            return 0
        fi
    fi

    tmp="$PUBLISH_WORKDIR/.verify-${asset_name}.$$"
    if gh release download "$RELEASE_TAG" \
        --repo "$REPO_FULL_NAME" \
        --pattern "$asset_name" \
        --output "$tmp" \
        --clobber >/dev/null 2>&1; then
        result="$(sha256_file "$tmp")"
        rm -f "$tmp"
        printf '%s' "$result"
        return 0
    fi
    rm -f "$tmp"
    return 1
}

# Populate LATEST_SOURCE_AS_OF and DESIRED_RELEASE_COUNT from discovery.
# DESIRED_RELEASE_COUNT is 1 + min(--history, available XLSX months), so a job
# asked for more history than the source has does not rebuild forever.
# Returns non-zero when discovery is unusable; callers then build.
compute_requested_coverage() {
    local json latest available wanted
    if ! json="$("$IDX_BIN" -o json ownership discover --family above1 --limit 50 2>/dev/null)"; then
        return 1
    fi

    latest="$(
        jq -r '
            if type == "array" and length > 0 and .[0].status == "supported"
            then (.[0].as_of_date // empty) else empty end
        ' <<< "$json" 2>/dev/null || true
    )"
    if ! [[ "$latest" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]]; then
        return 1
    fi

    available="$(
        jq -r --arg latest "$latest" '
            [ .[]
              | select(.status == "supported" and .format == "xlsx"
                       and .as_of_date != null and .as_of_date < $latest)
            ]
            | map(.as_of_date) | unique | length
        ' <<< "$json" 2>/dev/null || true
    )"
    if ! [[ "$available" =~ ^[0-9]+$ ]]; then
        return 1
    fi

    wanted="$HISTORY"
    if (( wanted > available )); then
        wanted="$available"
    fi
    LATEST_SOURCE_AS_OF="$latest"
    DESIRED_RELEASE_COUNT=$(( 1 + wanted ))
    return 0
}

STAGE="build"
if [[ "$BUILD_FIRST" == "1" ]]; then
    printf 'Building idx...\n'
    cargo build
fi

STAGE="preflight"
if ! "$IDX_BIN" version >/dev/null 2>&1; then
    fail 1 "failed to run idx binary: $IDX_BIN; build the CLI first or pass --idx-bin <path>"
fi

# Best effort read. A missing or unreadable manifest means "publish"; it is
# trusted for a no-op only after the referenced asset is verified.
PUBLISHED_MANIFEST="$(published_manifest)"
PUBLISHED_AS_OF="$(jq -r '.snapshot.latest_as_of_date // empty' <<< "$PUBLISHED_MANIFEST" 2>/dev/null || true)"
PUBLISHED_RELEASE_COUNT="$(jq -r '.snapshot.release_count // empty' <<< "$PUBLISHED_MANIFEST" 2>/dev/null || true)"
PUBLISHED_DOWNLOAD_URL="$(jq -r '.snapshot.download_url // empty' <<< "$PUBLISHED_MANIFEST" 2>/dev/null || true)"
PUBLISHED_SIZE_BYTES="$(jq -r '.snapshot.size_bytes // empty' <<< "$PUBLISHED_MANIFEST" 2>/dev/null || true)"
PUBLISHED_SHA256="$(jq -r '.snapshot.sqlite_sha256 // empty' <<< "$PUBLISHED_MANIFEST" 2>/dev/null || true)"
PUBLISHED_ASSET_NAME="${PUBLISHED_DOWNLOAD_URL##*/}"
PUBLISHED_TRUSTWORTHY="0"

if [[ "$PUBLISHED_AS_OF" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] \
    && [[ "$PUBLISHED_RELEASE_COUNT" =~ ^[0-9]+$ ]] \
    && [[ "$PUBLISHED_SIZE_BYTES" =~ ^[0-9]+$ ]] \
    && [[ "$PUBLISHED_SHA256" =~ ^[0-9a-fA-F]{64}$ ]] \
    && [[ -n "$PUBLISHED_ASSET_NAME" ]] \
    && published_url_matches "$PUBLISHED_DOWNLOAD_URL"; then
    if PUBLISHED_ASSETS_JSON="$(remote_assets_json)"; then
        published_asset_size="$(remote_asset_size "$PUBLISHED_ASSETS_JSON" "$PUBLISHED_ASSET_NAME")"
        if [[ "$published_asset_size" == "$PUBLISHED_SIZE_BYTES" ]]; then
            published_asset_sha="$(remote_asset_sha256 "$PUBLISHED_ASSETS_JSON" "$PUBLISHED_ASSET_NAME" 2>/dev/null)" || published_asset_sha=""
            if [[ "$published_asset_sha" == "$PUBLISHED_SHA256" ]]; then
                PUBLISHED_TRUSTWORTHY="1"
            fi
        fi
    fi
fi

LATEST_SOURCE_AS_OF=""
DESIRED_RELEASE_COUNT=""
if [[ "$FORCE" != "1" && "$PUBLISHED_TRUSTWORTHY" == "1" ]]; then
    STAGE="discover"
    if compute_requested_coverage; then
        if [[ "$LATEST_SOURCE_AS_OF" < "$PUBLISHED_AS_OF" ]]; then
            printf 'Published snapshot %s is newer than the latest source report %s; nothing to do.\n' \
                "$PUBLISHED_AS_OF" "$LATEST_SOURCE_AS_OF"
            printf 'RESULT: up-to-date %s\n' "$PUBLISHED_AS_OF"
            exit 0
        fi
        if [[ "$LATEST_SOURCE_AS_OF" == "$PUBLISHED_AS_OF" ]] \
            && (( DESIRED_RELEASE_COUNT <= PUBLISHED_RELEASE_COUNT )); then
            printf 'Published snapshot %s is current (latest source report: %s, releases: %s).\n' \
                "$PUBLISHED_AS_OF" "$LATEST_SOURCE_AS_OF" "$PUBLISHED_RELEASE_COUNT"
            printf 'RESULT: up-to-date %s\n' "$PUBLISHED_AS_OF"
            exit 0
        fi
        if [[ "$LATEST_SOURCE_AS_OF" == "$PUBLISHED_AS_OF" ]]; then
            printf 'Published snapshot %s has %s release(s) but history=%s needs %s; rebuilding.\n' \
                "$PUBLISHED_AS_OF" "$PUBLISHED_RELEASE_COUNT" "$HISTORY" "$DESIRED_RELEASE_COUNT"
        fi
    fi
fi

STAGE="build-snapshot"
build_args=(
    --idx-bin "$IDX_BIN"
    --output-dir "$PUBLISH_WORKDIR"
    --repo "$REPO_FULL_NAME"
    --release-tag "$RELEASE_TAG"
    --history "$HISTORY"
)

if [[ -n "$MAX_SNAPSHOT_BYTES" ]]; then
    build_args+=(--max-snapshot-bytes "$MAX_SNAPSHOT_BYTES")
fi

if [[ "$KEEP_WORKDIR" == "1" ]]; then
    build_args+=(--keep-workdir)
fi

printf 'Preparing latest ownership snapshot artifacts...\n'
"$BUILDER" "${build_args[@]}"

STAGED_MANIFEST_PATH="$PUBLISH_WORKDIR/ownership-snapshot-manifest.json"
if [[ ! -f "$STAGED_MANIFEST_PATH" ]]; then
    fail 1 "manifest was not generated: $STAGED_MANIFEST_PATH"
fi

shopt -s nullglob
sqlite_matches=("$PUBLISH_WORKDIR"/ownership-snapshot-*.sqlite)
shopt -u nullglob

if [[ "${#sqlite_matches[@]}" -ne 1 ]]; then
    fail 1 "expected exactly one SQLite artifact in $PUBLISH_WORKDIR"
fi

STAGED_SQLITE_PATH="${sqlite_matches[0]}"
BUILT_AS_OF="$(jq -r '.snapshot.latest_as_of_date // empty' "$STAGED_MANIFEST_PATH" 2>/dev/null || true)"
if ! [[ "$BUILT_AS_OF" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]]; then
    fail 1 "built manifest has no valid snapshot.latest_as_of_date: '$BUILT_AS_OF'"
fi
BUILT_RELEASE_COUNT="$(jq -r '.snapshot.release_count // empty' "$STAGED_MANIFEST_PATH" 2>/dev/null || true)"
if ! [[ "$BUILT_RELEASE_COUNT" =~ ^[0-9]+$ ]] || (( BUILT_RELEASE_COUNT < 1 )); then
    fail 1 "built manifest has no valid snapshot.release_count: '$BUILT_RELEASE_COUNT'"
fi

# Post-build check covers legacy PDF sources, whose as-of date is only known
# after import, and confirms that a history-only change is worth publishing.
if [[ "$FORCE" != "1" && "$PUBLISHED_TRUSTWORTHY" == "1" ]]; then
    if [[ "$BUILT_AS_OF" < "$PUBLISHED_AS_OF" ]] \
        || { [[ "$BUILT_AS_OF" == "$PUBLISHED_AS_OF" ]] && (( BUILT_RELEASE_COUNT <= PUBLISHED_RELEASE_COUNT )); }; then
        printf 'Built snapshot %s (releases: %s) is not newer than published %s (releases: %s); skipping upload.\n' \
            "$BUILT_AS_OF" "$BUILT_RELEASE_COUNT" "$PUBLISHED_AS_OF" "$PUBLISHED_RELEASE_COUNT"
        printf 'RESULT: up-to-date %s\n' "$PUBLISHED_AS_OF"
        exit 0
    fi
fi

STAGE="stage-output"

rm -f "$OUTPUT_DIR/ownership-snapshot-manifest.json"
shopt -s nullglob
existing_snapshot_paths=("$OUTPUT_DIR"/ownership-snapshot-*.sqlite)
shopt -u nullglob
if [[ "${#existing_snapshot_paths[@]}" -gt 0 ]]; then
    rm -f "${existing_snapshot_paths[@]}"
fi

cp "$STAGED_MANIFEST_PATH" "$OUTPUT_DIR/ownership-snapshot-manifest.json"
cp "$STAGED_SQLITE_PATH" "$OUTPUT_DIR/"

MANIFEST_PATH="$OUTPUT_DIR/ownership-snapshot-manifest.json"
SQLITE_PATH="$OUTPUT_DIR/$(basename "$STAGED_SQLITE_PATH")"
SQLITE_ASSET_NAME="$(basename "$STAGED_SQLITE_PATH")"
LOCAL_SQLITE_BYTES="$(wc -c < "$SQLITE_PATH" | tr -d ' ')"
LOCAL_SQLITE_SHA="$(jq -r '.snapshot.sqlite_sha256 // empty' "$MANIFEST_PATH" 2>/dev/null || true)"
if ! [[ "$LOCAL_SQLITE_SHA" =~ ^[0-9a-fA-F]{64}$ ]]; then
    fail 1 "staged manifest has no valid snapshot.sqlite_sha256: '$LOCAL_SQLITE_SHA'"
fi
LOCAL_FILE_SHA="$(sha256_file "$SQLITE_PATH")"
if [[ "$LOCAL_FILE_SHA" != "$LOCAL_SQLITE_SHA" ]]; then
    fail 1 "staged SQLite file hash $LOCAL_FILE_SHA does not match manifest sqlite_sha256 $LOCAL_SQLITE_SHA"
fi

STAGE="upload"
if gh release view "$RELEASE_TAG" --repo "$REPO_FULL_NAME" >/dev/null 2>&1; then
    printf 'Release %s already exists.\n' "$RELEASE_TAG"
else
    printf 'Creating stable snapshot release %s...\n' "$RELEASE_TAG"
    target_args=()
    # Packaged runs (Nix store) have no git checkout; let GitHub pick the default branch.
    if git_head="$(git rev-parse HEAD 2>/dev/null)"; then
        target_args=(--target "$git_head")
    fi
    gh release create "$RELEASE_TAG" \
        --repo "$REPO_FULL_NAME" \
        "${target_args[@]}" \
        --title "Ownership Snapshot Current" \
        --notes "Stable release for idx ownership snapshot artifacts consumed by \`idx ownership sync\`." \
        --latest=false
fi

# Upload the immutable SQLite artifact first. The content-addressed name makes a
# retry idempotent and keeps the previous valid artifact on --force replacements.
REMOTE_ASSETS_JSON="$(remote_assets_json)" || fail 1 "failed to read release assets for $RELEASE_TAG"
existing_size="$(remote_asset_size "$REMOTE_ASSETS_JSON" "$SQLITE_ASSET_NAME")"
existing_sha="$(remote_asset_sha256 "$REMOTE_ASSETS_JSON" "$SQLITE_ASSET_NAME" 2>/dev/null)" || existing_sha=""
if [[ "$existing_size" == "$LOCAL_SQLITE_BYTES" && "$existing_sha" == "$LOCAL_SQLITE_SHA" ]]; then
    printf 'SQLite asset already present and verified: %s\n' "$SQLITE_ASSET_NAME"
else
    printf 'Uploading SQLite asset %s...\n' "$SQLITE_ASSET_NAME"
    gh release upload "$RELEASE_TAG" "$SQLITE_PATH" \
        --repo "$REPO_FULL_NAME" \
        --clobber
fi

# Do not publish the manifest until the exact artifact is visible in the
# release and its full SHA-256 matches the staged manifest.
REMOTE_ASSETS_JSON="$(remote_assets_json)" || fail 1 "failed to re-read release assets for $RELEASE_TAG"
verified_size="$(remote_asset_size "$REMOTE_ASSETS_JSON" "$SQLITE_ASSET_NAME")"
verified_sha="$(remote_asset_sha256 "$REMOTE_ASSETS_JSON" "$SQLITE_ASSET_NAME" 2>/dev/null)" || verified_sha=""
if [[ "$verified_size" != "$LOCAL_SQLITE_BYTES" || "$verified_sha" != "$LOCAL_SQLITE_SHA" ]]; then
    fail 1 "SQLite asset integrity verification failed: expected $SQLITE_ASSET_NAME sha256=$LOCAL_SQLITE_SHA size=$LOCAL_SQLITE_BYTES in $RELEASE_TAG, found sha256='${verified_sha:-missing}' size='${verified_size:-missing}'"
fi

# The manifest is the consumer commit point, so it is published last.
STAGE="publish"
printf 'Publishing manifest ownership-snapshot-manifest.json...\n'
gh release upload "$RELEASE_TAG" "$MANIFEST_PATH" \
    --repo "$REPO_FULL_NAME" \
    --clobber

REMOTE_MANIFEST="$(published_manifest)"
REMOTE_MANIFEST_SHA="$(jq -r '.snapshot.sqlite_sha256 // empty' <<< "$REMOTE_MANIFEST" 2>/dev/null || true)"
if [[ -z "$LOCAL_SQLITE_SHA" || "$REMOTE_MANIFEST_SHA" != "$LOCAL_SQLITE_SHA" ]]; then
    fail 1 "published manifest verification failed: remote sqlite_sha256='${REMOTE_MANIFEST_SHA}' expected '$LOCAL_SQLITE_SHA'"
fi

printf 'Published manifest: https://github.com/%s/releases/download/%s/ownership-snapshot-manifest.json\n' \
    "$REPO_FULL_NAME" "$RELEASE_TAG"
printf 'Published SQLite: %s\n' "$SQLITE_PATH"
printf 'RESULT: published %s\n' "$BUILT_AS_OF"
