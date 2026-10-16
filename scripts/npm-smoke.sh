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

# Without postinstall (ignore-scripts, or a failed download) the wrapper
# installs the binary on first run, logging only to stderr: stdout is the MCP
# channel, and a client sees an early exit as "Connection closed".
echo "npm smoke: installing idx-mcp with --ignore-scripts"
no_scripts_dir="${smoke_dir}/idx-mcp-no-scripts"
npm_config_cache="$npm_cache" npm install --prefix "$no_scripts_dir" "${smoke_dir}"/idx-mcp/idx-mcp-*.tgz \
  --no-save --no-package-lock --no-audit --no-fund --ignore-scripts
# An empty user cache, so a binary cached by an earlier run cannot stand in
# for the first-run install.
XDG_CACHE_HOME="${smoke_dir}/no-scripts-cache" IDX_BINARY_URL="${repo_root}/target/release/idx-mcp" \
  "${no_scripts_dir}/node_modules/.bin/idx-mcp" --help > "${smoke_dir}/no-scripts-help.txt"
if ! head -n 1 "${smoke_dir}/no-scripts-help.txt" | grep -q "MCP server" \
  || [[ ! -x "${no_scripts_dir}/node_modules/idx-mcp/bin/idx-mcp" ]]; then
  echo "npm smoke: idx-mcp without postinstall did not start cleanly:" >&2
  cat "${smoke_dir}/no-scripts-help.txt" >&2
  exit 1
fi
# A read-only package (root-owned global install) falls back to the user cache.
echo "npm smoke: idx-mcp without postinstall, read-only package directory"
read_only_dir="${smoke_dir}/idx-mcp-read-only"
npm_config_cache="$npm_cache" npm install --prefix "$read_only_dir" "${smoke_dir}"/idx-mcp/idx-mcp-*.tgz \
  --no-save --no-package-lock --no-audit --no-fund --ignore-scripts
chmod -R a-w "${read_only_dir}/node_modules/idx-mcp"
XDG_CACHE_HOME="${smoke_dir}/user-cache" IDX_BINARY_URL="${repo_root}/target/release/idx-mcp" \
  "${read_only_dir}/node_modules/.bin/idx-mcp" --help > "${smoke_dir}/read-only-help.txt"
chmod -R u+w "${read_only_dir}/node_modules/idx-mcp"
cached=("${smoke_dir}"/user-cache/idx-mcp/*/*/idx-mcp)
if ! head -n 1 "${smoke_dir}/read-only-help.txt" | grep -q "MCP server" || [[ ! -x "${cached[0]}" ]]; then
  echo "npm smoke: idx-mcp with a read-only package did not install to the user cache:" >&2
  cat "${smoke_dir}/read-only-help.txt" >&2
  exit 1
fi
echo "npm smoke: PASS (idx-cli and idx-mcp: pack, install, --help; idx-mcp without postinstall, also read-only)"
