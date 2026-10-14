#!/usr/bin/env bash
set -Eeuo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
smoke_dir="$(mktemp -d "${TMPDIR:-/tmp}/idx-cli-npm-smoke.XXXXXX")"
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

# Packs the npm package in <dir>, installs it with the local release binary,
# and checks that `<bin> --help` prints `<usage>`.
smoke_package() {
  local package_dir="$1" bin="$2" usage="$3"
  local release_binary="${repo_root}/target/release/${bin}"
  local work="${smoke_dir}/${bin}"
  local install_dir="${work}/install"
  mkdir -p "$work" "$install_dir"

  if [[ ! -x "$release_binary" ]]; then
    echo "npm smoke: release binary was not produced at ${release_binary}" >&2
    exit 1
  fi

  echo "npm smoke: packing ${package_dir}"
  local tarball_name tarball
  tarball_name="$(cd "$package_dir" && npm_config_cache="$npm_cache" npm pack --silent --pack-destination "$work" | tail -n 1)"
  tarball="${work}/${tarball_name}"
  if [[ -z "$tarball_name" || ! -f "$tarball" ]]; then
    echo "npm smoke: expected tarball was not produced at ${tarball}" >&2
    exit 1
  fi

  local binary_url
  binary_url="$(node -e 'const { pathToFileURL } = require("node:url"); process.stdout.write(pathToFileURL(process.argv[1]).href)' "$release_binary")"
  echo "npm smoke: installing ${tarball_name} into ${install_dir}"
  npm_config_cache="$npm_cache" IDX_BINARY_URL="$binary_url" npm install \
    --prefix "$install_dir" \
    "$tarball" \
    --no-save \
    --no-package-lock \
    --no-audit \
    --no-fund \
    --foreground-scripts

  local resolved_bin="${install_dir}/node_modules/.bin/${bin}"
  if [[ ! -x "$resolved_bin" ]]; then
    echo "npm smoke: npm did not create an executable bin link at ${resolved_bin}" >&2
    exit 1
  fi

  echo "npm smoke: running installed ${bin} --help"
  "$resolved_bin" --help | tee "${work}/help.txt"
  if ! grep -q "$usage" "${work}/help.txt"; then
    echo "npm smoke: ${bin} --help did not contain '${usage}'" >&2
    exit 1
  fi
}

if [[ -x target/release/idx && -x target/release/idx-mcp ]]; then
  echo "npm smoke: reusing target/release/{idx,idx-mcp}"
else
  echo "npm smoke: building release binaries"
  cargo build --release --locked -p idx-cli -p idx-mcp
fi

mkdir -p "$npm_cache"
smoke_package "$repo_root" idx "Usage: idx"
"${smoke_dir}/idx/install/node_modules/.bin/idx" config --help >/dev/null
smoke_package "${repo_root}/npm/idx-mcp" idx-mcp "Usage: idx-mcp"
echo "npm smoke: PASS (idx-cli and idx-mcp: pack, install, --help)"
