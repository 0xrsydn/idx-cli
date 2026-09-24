#!/usr/bin/env bash
# Tests install.sh against a local fake GitHub release server (no live network).
#   scripts/install-sh-test.sh            # runs install.sh with sh
#   INSTALL_SH_SHELL=dash scripts/install-sh-test.sh
set -Eeuo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
installer="${repo_root}/install.sh"
test_shell="${INSTALL_SH_SHELL:-sh}"
work="$(mktemp -d "${TMPDIR:-/tmp}/idx-install-test.XXXXXX")"
server_pid=

cleanup() {
  if [[ -n "$server_pid" ]]; then
    kill "$server_pid" 2>/dev/null || true
  fi
  rm -rf "$work"
}
trap cleanup EXIT

failures=0
pass() { echo "ok   - $1"; }
fail() {
  echo "FAIL - $1" >&2
  failures=$((failures + 1))
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d ' ' -f 1
  else
    shasum -a 256 "$1" | cut -d ' ' -f 1
  fi
}

case "$(uname -s)" in
  Linux) os=linux ;;
  Darwin) os=darwin ;;
  *) echo "unsupported test host" >&2; exit 1 ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) arch=x64 ;;
  *) arch=arm64 ;;
esac
if [[ "$os" == darwin && "$arch" == x64 && "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" == 1 ]]; then
  arch=arm64
fi
asset="idx-${os}-${arch}"

# Fake release layout served by python's http.server:
#   /api/repos/0xrsydn/idx-cli/releases              (query string is ignored)
#   /gh/0xrsydn/idx-cli/releases/download/<tag>/...
root="${work}/srv"
api_dir="${root}/api/repos/0xrsydn/idx-cli"
mkdir -p "$api_dir"
cat >"${api_dir}/releases" <<'EOF'
[
  {
    "tag_name": "ownership-snapshot-current",
    "name": "Ownership Snapshot Current"
  },
  {
    "tag_name": "v0.2.4",
    "name": "v0.2.4"
  },
  {
    "tag_name": "v0.2.3",
    "name": "v0.2.3"
  }
]
EOF

make_release() {
  local tag="$1" dir="${root}/gh/0xrsydn/idx-cli/releases/download/$1"
  mkdir -p "$dir"
  printf '#!/bin/sh\necho "idx fake %s"\n' "$tag" >"${dir}/${asset}"
  chmod +x "${dir}/${asset}"
  (cd "$dir" && echo "$(sha256 "$asset")  ${asset}" >SHA256SUMS)
}
make_release v0.2.3
make_release v0.2.4

# v0.0.9: checksum does not match the binary.
bad="${root}/gh/0xrsydn/idx-cli/releases/download/v0.0.9"
mkdir -p "$bad"
printf '#!/bin/sh\necho tampered\n' >"${bad}/${asset}"
printf '%064d  %s\n' 0 "$asset" >"${bad}/SHA256SUMS"

port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')"
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$root" >/dev/null 2>&1 &
server_pid=$!
for _ in $(seq 50); do
  curl -fs "http://127.0.0.1:${port}/api/repos/0xrsydn/idx-cli/releases" >/dev/null 2>&1 && break
  sleep 0.1
done

run_installer() {
  env IDX_GITHUB_URL="http://127.0.0.1:${port}/gh" \
    IDX_GITHUB_API="http://127.0.0.1:${port}/api" \
    HOME="${work}/home" \
    "$@"
}

# 1. latest resolves to the newest vX.Y.Z tag, skipping non-app releases.
dir="${work}/latest"
if run_installer "$test_shell" "$installer" --dir "$dir" >"${work}/out1" 2>&1 &&
  [[ "$("${dir}/idx")" == "idx fake v0.2.4" ]]; then
  pass "installs latest vX.Y.Z release"
else
  fail "installs latest vX.Y.Z release"; cat "${work}/out1" >&2
fi

# 2. pinned version via env, without the leading v.
dir="${work}/pinned"
if run_installer env IDX_VERSION=0.2.3 IDX_INSTALL_DIR="$dir" "$test_shell" "$installer" >"${work}/out2" 2>&1 &&
  [[ "$("${dir}/idx")" == "idx fake v0.2.3" ]]; then
  pass "installs pinned version from IDX_VERSION"
else
  fail "installs pinned version from IDX_VERSION"; cat "${work}/out2" >&2
fi

# 3. piped through stdin like `curl ... | sh -s -- --version v0.2.3`.
dir="${work}/piped"
if run_installer "$test_shell" -s -- --version v0.2.3 --dir "$dir" <"$installer" >"${work}/out3" 2>&1 &&
  [[ "$("${dir}/idx")" == "idx fake v0.2.3" ]]; then
  pass "works when piped to sh -s --"
else
  fail "works when piped to sh -s --"; cat "${work}/out3" >&2
fi

# 4. checksum mismatch is rejected and nothing is installed.
dir="${work}/bad"
if run_installer "$test_shell" "$installer" --version v0.0.9 --dir "$dir" >"${work}/out4" 2>&1; then
  fail "rejects checksum mismatch (installer succeeded)"
elif grep -q "checksum mismatch" "${work}/out4" && [[ ! -e "${dir}/idx" ]]; then
  pass "rejects checksum mismatch"
else
  fail "rejects checksum mismatch"; cat "${work}/out4" >&2
fi

# 5. missing release reports a clear download error.
if run_installer "$test_shell" "$installer" --version v9.9.9 --dir "${work}/missing" >"${work}/out5" 2>&1; then
  fail "reports missing release (installer succeeded)"
elif grep -q "download failed" "${work}/out5"; then
  pass "reports missing release"
else
  fail "reports missing release"; cat "${work}/out5" >&2
fi

# 6. unsupported platform, via a uname shim on PATH.
shim="${work}/shim"
mkdir -p "$shim"
# shellcheck disable=SC2016 # literal $1 belongs to the shim
printf '#!/bin/sh\ncase "$1" in -s) echo FreeBSD ;; *) echo amd64 ;; esac\n' >"${shim}/uname"
chmod +x "${shim}/uname"
if run_installer env PATH="${shim}:${PATH}" "$test_shell" "$installer" --dir "${work}/bsd" >"${work}/out6" 2>&1; then
  fail "rejects unsupported OS (installer succeeded)"
elif grep -q "unsupported OS: FreeBSD" "${work}/out6"; then
  pass "rejects unsupported OS"
else
  fail "rejects unsupported OS"; cat "${work}/out6" >&2
fi

# 7. PATH hint when the install dir is not on PATH.
if grep -q "is not on your PATH" "${work}/out1"; then
  pass "prints PATH hint"
else
  fail "prints PATH hint"; cat "${work}/out1" >&2
fi

# 8. reinstall over an existing binary replaces it.
dir="${work}/latest"
if run_installer "$test_shell" "$installer" --version v0.2.3 --dir "$dir" >"${work}/out8" 2>&1 &&
  [[ "$("${dir}/idx")" == "idx fake v0.2.3" ]] &&
  [[ -z "$(find "$dir" -name '.idx.tmp.*')" ]]; then
  pass "replaces an existing install cleanly"
else
  fail "replaces an existing install cleanly"; cat "${work}/out8" >&2
fi

# 9. quoted ~ in --dir expands to HOME; trailing slash on mirror URLs is fine.
# shellcheck disable=SC2088 # the literal ~ is the input under test
if env IDX_GITHUB_URL="http://127.0.0.1:${port}/gh/" \
  IDX_GITHUB_API="http://127.0.0.1:${port}/api/" \
  HOME="${work}/home" \
  "$test_shell" "$installer" --dir "~/tilde-bin" >"${work}/out9" 2>&1 &&
  [[ -x "${work}/home/tilde-bin/idx" ]]; then
  pass "expands quoted ~ and tolerates trailing slashes"
else
  fail "expands quoted ~ and tolerates trailing slashes"; cat "${work}/out9" >&2
fi

if ((failures > 0)); then
  echo "install.sh tests: ${failures} failure(s) with ${test_shell}" >&2
  exit 1
fi
echo "install.sh tests: all passed with ${test_shell}"
