#!/bin/sh
# idx-cli installer: downloads a prebuilt `idx` binary from GitHub Releases,
# verifies it against the release SHA256SUMS, and installs it without sudo.
#
#   curl -fsSL https://raw.githubusercontent.com/0xrsydn/idx-cli/main/install.sh | sh
#   curl -fsSL https://raw.githubusercontent.com/0xrsydn/idx-cli/main/install.sh | sh -s -- --version v0.2.3
#
# Environment (flags take precedence):
#   IDX_VERSION       release tag to install (default: latest vX.Y.Z release)
#   IDX_INSTALL_DIR   install directory (default: $HOME/.local/bin)
#   IDX_GITHUB_URL    GitHub base URL, for mirrors (default: https://github.com)
#   IDX_GITHUB_API    GitHub API base URL, for mirrors (default: https://api.github.com)

set -eu

REPO="0xrsydn/idx-cli"
BIN_NAME="idx"

say() {
    printf 'idx-install: %s\n' "$*"
}

err() {
    printf 'idx-install: error: %s\n' "$*" >&2
    exit 1
}

usage() {
    cat <<'EOF'
Install the idx CLI from GitHub Releases.

Usage: install.sh [--version <tag>] [--dir <path>]

Options:
  --version <tag>  Release tag to install, for example v0.2.3 (default: latest)
  --dir <path>     Install directory (default: $HOME/.local/bin)
  -h, --help       Show this help

Environment: IDX_VERSION, IDX_INSTALL_DIR, IDX_GITHUB_URL, IDX_GITHUB_API
EOF
}

has() {
    command -v "$1" >/dev/null 2>&1
}

# fetch <url> <output-file>
fetch() {
    if has curl; then
        curl --fail --silent --show-error --location --retry 3 --output "$2" "$1"
    elif has wget; then
        wget --quiet --tries=3 --output-document="$2" "$1"
    else
        err "need curl or wget to download files"
    fi
}

sha256_of() {
    if has sha256sum; then
        sha256sum "$1" | cut -d ' ' -f 1
    elif has shasum; then
        shasum -a 256 "$1" | cut -d ' ' -f 1
    else
        err "need sha256sum or shasum to verify the download"
    fi
}

detect_asset() {
    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux) os=linux ;;
        Darwin) os=darwin ;;
        *) err "unsupported OS: $os (supported: Linux, macOS; on Windows use WSL)" ;;
    esac

    case "$arch" in
        x86_64 | amd64) arch=x64 ;;
        aarch64 | arm64) arch=arm64 ;;
        *) err "unsupported CPU architecture: $arch (supported: x86_64, arm64)" ;;
    esac

    # An x64 shell under Rosetta on Apple Silicon should still get the native build.
    if [ "$os" = darwin ] && [ "$arch" = x64 ] &&
        [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then
        arch=arm64
    fi

    echo "idx-${os}-${arch}"
}

# Pick the newest vX.Y.Z tag. The repo also carries non-app releases such as
# ownership-snapshot-current, so /releases/latest is not reliable here.
resolve_latest_version() {
    releases="$tmp_dir/releases.json"
    fetch "$api_url/repos/$REPO/releases?per_page=30" "$releases" ||
        err "could not list releases; pass --version <tag> to skip the lookup"

    tag="$(
        sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\(v[0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)".*/\1/p' "$releases" |
            head -n 1
    )"
    [ -n "$tag" ] || err "no vX.Y.Z release found for $REPO"
    echo "$tag"
}

os_release_id() {
    if [ -r /etc/os-release ]; then
        # shellcheck source=/dev/null
        (. /etc/os-release && echo "${ID:-}")
    fi
}

mutool_hint() {
    if [ "$(uname -s)" = Darwin ]; then
        echo "brew install mupdf"
        return
    fi
    case "$(os_release_id)" in
        arch | manjaro | endeavouros) echo "sudo pacman -S mupdf-tools" ;;
        debian | ubuntu | linuxmint | pop) echo "sudo apt install mupdf-tools" ;;
        fedora) echo "sudo dnf install mupdf" ;;
        alpine) echo "sudo apk add mupdf-tools" ;;
        opensuse* | sles) echo "sudo zypper install mupdf" ;;
        *) echo "install MuPDF's mutool with your package manager" ;;
    esac
}

print_notes() {
    if [ -e /etc/NIXOS ] || [ "$(os_release_id)" = nixos ]; then
        say "NixOS detected: the flake wraps idx with its helper tools and may suit you better:"
        say "  nix profile install github:$REPO#default"
    fi

    if ! has mutool; then
        say "optional: 'idx ownership import' needs mutool from MuPDF: $(mutool_hint)"
    fi

    found_curl_impersonate=
    for candidate in curl_chrome142 curl_chrome136 curl_chrome133a curl_chrome131 curl_chrome124 curl_chrome120 curl_chrome116; do
        if has "$candidate"; then
            found_curl_impersonate=1
            break
        fi
    done
    if [ -z "$found_curl_impersonate" ] && [ -z "${IDX_CURL_IMPERSONATE_BIN:-}" ]; then
        say "optional: Yahoo-backed commands need a curl_chrome* binary from curl-impersonate"
        say "  (https://github.com/lexiforest/curl-impersonate) or IDX_CURL_IMPERSONATE_BIN"
    fi
}

print_path_hint() {
    case ":$PATH:" in
        *":$install_dir:"*) return ;;
    esac

    case "${SHELL:-}" in
        */fish) line="fish_add_path $install_dir" ;;
        */zsh) line="echo 'export PATH=\"$install_dir:\$PATH\"' >> ~/.zshrc" ;;
        *) line="echo 'export PATH=\"$install_dir:\$PATH\"' >> ~/.bashrc" ;;
    esac
    say "$install_dir is not on your PATH. Add it with:"
    say "  $line"
}

main() {
    version="${IDX_VERSION:-}"
    install_dir="${IDX_INSTALL_DIR:-${HOME:?HOME is not set}/.local/bin}"
    github_url="${IDX_GITHUB_URL:-https://github.com}"
    api_url="${IDX_GITHUB_API:-https://api.github.com}"

    while [ $# -gt 0 ]; do
        case "$1" in
            --version)
                [ $# -ge 2 ] || err "--version needs a value"
                version="$2"
                shift 2
                ;;
            --dir)
                [ $# -ge 2 ] || err "--dir needs a value"
                install_dir="$2"
                shift 2
                ;;
            -h | --help)
                usage
                return 0
                ;;
            *) err "unknown argument: $1 (see --help)" ;;
        esac
    done

    # A quoted --dir "~/bin" or IDX_INSTALL_DIR="~/bin" reaches us unexpanded.
    # shellcheck disable=SC2088 # matching a literal, unexpanded ~ on purpose
    case "$install_dir" in
        "~") install_dir="$HOME" ;;
        "~/"*) install_dir="$HOME/${install_dir#"~/"}" ;;
    esac
    github_url="${github_url%/}"
    api_url="${api_url%/}"

    asset="$(detect_asset)"

    tmp_dir="$(mktemp -d 2>/dev/null || mktemp -d -t idx-install)"
    trap 'rm -rf "$tmp_dir"' EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM

    if [ -z "$version" ]; then
        version="$(resolve_latest_version)"
    fi
    case "$version" in
        v*) ;;
        *) version="v$version" ;;
    esac

    download_base="$github_url/$REPO/releases/download/$version"
    say "installing idx $version ($asset) into $install_dir"

    fetch "$download_base/$asset" "$tmp_dir/$asset" ||
        err "download failed: $download_base/$asset (does release $version have binaries?)"
    fetch "$download_base/SHA256SUMS" "$tmp_dir/SHA256SUMS" ||
        err "download failed: $download_base/SHA256SUMS"

    expected="$(awk -v name="$asset" '$2 == name || $2 == "*" name { print $1; exit }' "$tmp_dir/SHA256SUMS")"
    [ -n "$expected" ] || err "SHA256SUMS for $version has no entry for $asset"
    actual="$(sha256_of "$tmp_dir/$asset")"
    [ "$actual" = "$expected" ] ||
        err "checksum mismatch for $asset: expected $expected, got $actual"

    chmod 0755 "$tmp_dir/$asset"
    "$tmp_dir/$asset" version >/dev/null 2>&1 ||
        err "downloaded binary does not run on this system"

    mkdir -p "$install_dir" || err "cannot create $install_dir"
    staged="$install_dir/.$BIN_NAME.tmp.$$"
    if ! { cp "$tmp_dir/$asset" "$staged" && chmod 0755 "$staged" && mv -f "$staged" "$install_dir/$BIN_NAME"; }; then
        rm -f "$staged"
        err "cannot write $install_dir/$BIN_NAME"
    fi

    say "installed $install_dir/$BIN_NAME"

    existing="$(command -v "$BIN_NAME" 2>/dev/null || true)"
    if [ -n "$existing" ] && [ "$existing" != "$install_dir/$BIN_NAME" ]; then
        say "warning: another idx at $existing comes first on your PATH"
    fi

    print_path_hint
    print_notes
    say "done; run 'idx --help' to get started"
}

# Everything runs from main so a truncated download of this script does nothing.
main "$@"
