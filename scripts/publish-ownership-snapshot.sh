#!/usr/bin/env bash

set -Eeuo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/publish-ownership-snapshot.sh --output-dir <dir> [options]

Build and publish the latest supported IDX/KSEI ownership snapshot to the stable
GitHub release used by `idx ownership sync`.

Safe to run daily: unless --force is given, it skips the upload when the
published snapshot already has the latest as-of date. The last line is always
one of:
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
  --history <n>          Also include the <n> previous monthly above-1% reports
                         in the snapshot (passed to the builder; default: 0)
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
FORCE="0"
PUBLISH_WORKDIR=""
STAGE="arguments"

on_error() {
    local status=$?
    printf 'RESULT: FAILED stage=%s (exit %s)\n' "$STAGE" "$status" >&2
    exit "$status"
}
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

if ! command -v gh >/dev/null 2>&1; then
    echo "gh is required for GitHub release upload" >&2
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILDER="$SCRIPT_DIR/build-latest-ownership-snapshot.sh"

if [[ ! -x "$BUILDER" ]]; then
    echo "required helper script is missing or not executable: $BUILDER" >&2
    exit 1
fi

mkdir -p "$OUTPUT_DIR"

PUBLISH_WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/idx-ownership-publish.XXXXXX")"
cleanup() {
    if [[ -z "$PUBLISH_WORKDIR" ]]; then
        return
    fi

    if [[ "$KEEP_WORKDIR" == "1" ]]; then
        printf 'Kept publish workdir: %s\n' "$PUBLISH_WORKDIR"
        return
    fi

    rm -rf "$PUBLISH_WORKDIR"
}
trap cleanup EXIT

# Published manifest, or empty when none exists yet.
published_manifest() {
    gh release download "$RELEASE_TAG" \
        --repo "$REPO_FULL_NAME" \
        --pattern ownership-snapshot-manifest.json \
        --output - 2>/dev/null || true
}

STAGE="build"
if [[ "$BUILD_FIRST" == "1" ]]; then
    printf 'Building idx...\n'
    cargo build
fi

if ! "$IDX_BIN" version >/dev/null 2>&1; then
    echo "failed to run idx binary: $IDX_BIN" >&2
    echo "build the CLI first or pass --idx-bin <path>" >&2
    exit 1
fi

STAGE="check-published"
PUBLISHED_MANIFEST="$(published_manifest)"
PUBLISHED_AS_OF="$(jq -r '.snapshot.latest_as_of_date // empty' <<< "$PUBLISHED_MANIFEST" 2>/dev/null || true)"

if [[ "$FORCE" != "1" && -n "$PUBLISHED_AS_OF" ]]; then
    STAGE="discover"
    # Cheap pre-check: XLSX reports carry their as-of date, so an unchanged
    # month is detected without downloading or building anything.
    LATEST_AS_OF="$(
        "$IDX_BIN" -o json ownership discover --family above1 --limit 1 |
            jq -r '.[0] | select(.status == "supported") | .as_of_date // empty'
    )"
    if [[ -n "$LATEST_AS_OF" && ! "$LATEST_AS_OF" > "$PUBLISHED_AS_OF" ]]; then
        printf 'Published snapshot %s is current (latest source report: %s).\n' \
            "$PUBLISHED_AS_OF" "$LATEST_AS_OF"
        printf 'RESULT: up-to-date %s\n' "$PUBLISHED_AS_OF"
        exit 0
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

if [[ "$KEEP_WORKDIR" == "1" ]]; then
    build_args+=(--keep-workdir)
fi

printf 'Preparing latest ownership snapshot artifacts...\n'
"$BUILDER" "${build_args[@]}"

STAGED_MANIFEST_PATH="$PUBLISH_WORKDIR/ownership-snapshot-manifest.json"
if [[ ! -f "$STAGED_MANIFEST_PATH" ]]; then
    echo "manifest was not generated: $STAGED_MANIFEST_PATH" >&2
    exit 1
fi

shopt -s nullglob
sqlite_matches=("$PUBLISH_WORKDIR"/ownership-snapshot-*.sqlite)
existing_snapshot_paths=("$OUTPUT_DIR"/ownership-snapshot-*.sqlite)
shopt -u nullglob

if [[ "${#sqlite_matches[@]}" -ne 1 ]]; then
    echo "expected exactly one SQLite artifact in $PUBLISH_WORKDIR" >&2
    exit 1
fi

STAGED_SQLITE_PATH="${sqlite_matches[0]}"
BUILT_AS_OF="$(jq -r '.snapshot.latest_as_of_date' "$STAGED_MANIFEST_PATH")"

# Post-build check covers legacy PDF sources, whose as-of date is only known
# after import.
if [[ "$FORCE" != "1" && -n "$PUBLISHED_AS_OF" && ! "$BUILT_AS_OF" > "$PUBLISHED_AS_OF" ]]; then
    printf 'Built snapshot %s is not newer than published %s; skipping upload.\n' \
        "$BUILT_AS_OF" "$PUBLISHED_AS_OF"
    printf 'RESULT: up-to-date %s\n' "$PUBLISHED_AS_OF"
    exit 0
fi

STAGE="stage-output"

rm -f "$OUTPUT_DIR/ownership-snapshot-manifest.json"
if [[ "${#existing_snapshot_paths[@]}" -gt 0 ]]; then
    rm -f "${existing_snapshot_paths[@]}"
fi

cp "$STAGED_MANIFEST_PATH" "$OUTPUT_DIR/ownership-snapshot-manifest.json"
cp "$STAGED_SQLITE_PATH" "$OUTPUT_DIR/"

MANIFEST_PATH="$OUTPUT_DIR/ownership-snapshot-manifest.json"
SQLITE_PATH="$OUTPUT_DIR/$(basename "$STAGED_SQLITE_PATH")"

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

printf 'Uploading snapshot assets to %s...\n' "$RELEASE_TAG"
gh release upload "$RELEASE_TAG" \
    "$MANIFEST_PATH" \
    "$SQLITE_PATH" \
    --repo "$REPO_FULL_NAME" \
    --clobber

printf 'Published manifest: https://github.com/%s/releases/download/%s/ownership-snapshot-manifest.json\n' \
    "$REPO_FULL_NAME" "$RELEASE_TAG"
printf 'Published SQLite: %s\n' "$SQLITE_PATH"
printf 'RESULT: published %s\n' "$BUILT_AS_OF"
