#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:-}"
if [[ -z "$VERSION" ]]; then
  if git describe --tags --always >/dev/null 2>&1; then
    VERSION="$(git describe --tags --always)"
  else
    VERSION="0.0.0-$(date +%Y%m%d%H%M%S)"
  fi
fi

APP_NAME="rubick"
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$ARCH" in
  x86_64) ARCH="amd64" ;;
  aarch64|arm64) ARCH="arm64" ;;
esac

DIST_DIR="dist"
PKG_BASENAME="${APP_NAME}_${VERSION}_${OS}_${ARCH}"
PKG_DIR="${DIST_DIR}/${PKG_BASENAME}"

rm -rf "$PKG_DIR"
mkdir -p "$PKG_DIR/bin" "$PKG_DIR/scripts"

# Build binary
GOOS="$OS" GOARCH="$ARCH" go build -o "$PKG_DIR/bin/$APP_NAME" ./cmd/rubick

# Runtime Python assets
cp extractor.py "$PKG_DIR/"
cp scripts/export_dashboard.py scripts/export_history.py scripts/export_simple.py "$PKG_DIR/scripts/"
cp pyproject.toml uv.lock .env.example README.md "$PKG_DIR/"

cat > "$PKG_DIR/INSTALL.md" <<'DOC'
# Rubick Bundle Install

1. Ensure Python 3.12+ and uv are installed.
2. In this bundle directory, run:

```bash
uv sync --frozen
```

3. Run the binary:

```bash
./bin/rubick --help
```

4. For live news queries, create `.env` with `BRAVE_API_KEY`.
DOC

# Checksums
( cd "$PKG_DIR" && shasum -a 256 bin/$APP_NAME extractor.py scripts/*.py pyproject.toml uv.lock > SHA256SUMS )

# Archives
( cd "$DIST_DIR" && tar -czf "${PKG_BASENAME}.tar.gz" "$PKG_BASENAME" )
( cd "$DIST_DIR" && zip -qr "${PKG_BASENAME}.zip" "$PKG_BASENAME" )

echo "release_bundle=$PKG_DIR"
echo "archive_tar=${DIST_DIR}/${PKG_BASENAME}.tar.gz"
echo "archive_zip=${DIST_DIR}/${PKG_BASENAME}.zip"
