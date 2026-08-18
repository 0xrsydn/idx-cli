#!/usr/bin/env bash
set -Eeuo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
smoke_dir="$(mktemp -d "${TMPDIR:-/tmp}/idx-cli-npm-smoke.XXXXXX")"
tarball="${smoke_dir}/idx-cli-0.2.3.tgz"
install_dir="${smoke_dir}/install"
npm_cache="${smoke_dir}/npm-cache"

cleanup() {
  rm -rf "$smoke_dir"
}

on_error() {
  local status=$?
  echo "npm smoke: FAILED on line ${BASH_LINENO[0]} (status ${status})" >&2
  exit "$status"
}

trap cleanup EXIT
trap on_error ERR

cd "$repo_root"

release_binary="${repo_root}/target/release/idx"
if [[ -x "$release_binary" ]]; then
  echo "npm smoke: reusing ${release_binary}"
else
  echo "npm smoke: building ${release_binary}"
  cargo build --release --locked
fi

if [[ ! -x "$release_binary" ]]; then
  echo "npm smoke: release binary was not produced at ${release_binary}" >&2
  exit 1
fi

echo "npm smoke: packing wrapper"
mkdir -p "$npm_cache"
npm_config_cache="$npm_cache" npm pack --silent --pack-destination "$smoke_dir" >/dev/null
if [[ ! -f "$tarball" ]]; then
  echo "npm smoke: expected tarball was not produced at ${tarball}" >&2
  exit 1
fi

mkdir -p "$install_dir"
binary_url="$(node -e 'const { pathToFileURL } = require("node:url"); process.stdout.write(pathToFileURL(process.argv[1]).href)' "$release_binary")"
echo "npm smoke: installing tarball into ${install_dir}"
npm_config_cache="$npm_cache" IDX_BINARY_URL="$binary_url" npm install \
  --prefix "$install_dir" \
  "$tarball" \
  --no-save \
  --no-package-lock \
  --no-audit \
  --no-fund \
  --foreground-scripts

resolved_bin="${install_dir}/node_modules/.bin/idx"
if [[ ! -x "$resolved_bin" ]]; then
  echo "npm smoke: npm did not create an executable bin link at ${resolved_bin}" >&2
  exit 1
fi

help_output="${smoke_dir}/help.txt"
echo "npm smoke: running installed idx --help"
"$resolved_bin" --help | tee "$help_output"
if ! rg -q "Usage: idx" "$help_output"; then
  echo "npm smoke: --help output did not contain the expected usage line" >&2
  exit 1
fi

echo "npm smoke: running installed idx config --help"
"$resolved_bin" config --help
echo "npm smoke: PASS (pack, install, --help, and config --help)"
