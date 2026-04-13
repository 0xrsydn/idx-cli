#!/usr/bin/env bash

set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/publish-ownership-snapshot.sh --output-dir <dir> [options]

Build and publish the latest supported IDX/KSEI ownership snapshot to the stable
GitHub release used by `idx ownership sync`.

Options:
  --idx-bin <path>       idx binary to use (default: ./target/debug/idx)
  --output-dir <dir>     Directory to write the copied snapshot and manifest
  --repo <owner/name>    GitHub repo used for release upload and public URLs
                         (default: 0xrsydn/idx-cli)
  --release-tag <tag>    Stable GitHub release tag used for snapshot assets
                         (default: ownership-snapshot-current)
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

if [[ "$BUILD_FIRST" == "1" ]]; then
    printf 'Building idx...\n'
    cargo build
fi

if ! "$IDX_BIN" version >/dev/null 2>&1; then
    echo "failed to run idx binary: $IDX_BIN" >&2
    echo "build the CLI first or pass --idx-bin <path>" >&2
    exit 1
fi

build_args=(
    --idx-bin "$IDX_BIN"
    --output-dir "$OUTPUT_DIR"
    --repo "$REPO_FULL_NAME"
    --release-tag "$RELEASE_TAG"
)

if [[ "$KEEP_WORKDIR" == "1" ]]; then
    build_args+=(--keep-workdir)
fi

printf 'Preparing latest ownership snapshot artifacts...\n'
"$BUILDER" "${build_args[@]}"

MANIFEST_PATH="$OUTPUT_DIR/ownership-snapshot-manifest.json"
if [[ ! -f "$MANIFEST_PATH" ]]; then
    echo "manifest was not generated: $MANIFEST_PATH" >&2
    exit 1
fi

shopt -s nullglob
sqlite_matches=("$OUTPUT_DIR"/ownership-snapshot-*.sqlite)
shopt -u nullglob

if [[ "${#sqlite_matches[@]}" -ne 1 ]]; then
    echo "expected exactly one SQLite artifact in $OUTPUT_DIR" >&2
    exit 1
fi

SQLITE_PATH="${sqlite_matches[0]}"

if gh release view "$RELEASE_TAG" --repo "$REPO_FULL_NAME" >/dev/null 2>&1; then
    printf 'Release %s already exists.\n' "$RELEASE_TAG"
else
    printf 'Creating stable snapshot release %s...\n' "$RELEASE_TAG"
    gh release create "$RELEASE_TAG" \
        --repo "$REPO_FULL_NAME" \
        --target "$(git rev-parse HEAD)" \
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
