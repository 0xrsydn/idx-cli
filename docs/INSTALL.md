# Install script

`install.sh` installs a prebuilt `idx` binary from GitHub Releases. It is the
fastest path on Linux and macOS and needs no Rust toolchain, Node, or sudo.

```bash
curl -fsSL https://raw.githubusercontent.com/0xrsydn/idx-cli/main/install.sh | sh
```

Each release also carries a copy of the script as a release asset, so a
versioned URL stays stable:

```bash
curl -fsSL https://github.com/0xrsydn/idx-cli/releases/download/v0.2.4/install.sh | sh
```

## What it does

1. Detects the OS and CPU with `uname` (Rosetta shells on Apple Silicon get
   the native arm64 build).
2. Resolves the newest `vX.Y.Z` release through the GitHub API. It does not use
   `/releases/latest`, because this repo also publishes non-app releases such
   as `ownership-snapshot-current`.
3. Downloads the binary and `SHA256SUMS`, and refuses to install on a checksum
   mismatch.
4. Runs `idx version` from the download to confirm it executes here.
5. Moves the binary into the install directory in one step, replacing any
   previous install.
6. Prints a PATH hint when needed, plus notes about optional helper tools.

It never edits shell profiles and never calls a system package manager. On
NixOS it suggests the flake instead, and when `mutool` or `curl_chrome*` are
missing it prints the matching package-manager command so you can install
them yourself.

## Options

| Flag | Environment | Default |
| --- | --- | --- |
| `--version <tag>` | `IDX_VERSION` | newest `vX.Y.Z` release |
| `--dir <path>` | `IDX_INSTALL_DIR` | `$HOME/.local/bin` |
| | `IDX_GITHUB_URL` | `https://github.com` (for mirrors) |
| | `IDX_GITHUB_API` | `https://api.github.com` (for mirrors) |

Pass flags through the pipe with `sh -s --`:

```bash
curl -fsSL https://raw.githubusercontent.com/0xrsydn/idx-cli/main/install.sh | sh -s -- --version v0.2.3 --dir ~/bin
```

## Supported platforms

Same assets as the npm package: `idx-linux-x64`, `idx-linux-arm64` (static
musl builds), `idx-darwin-arm64`, `idx-darwin-x64`. Windows users can run the
script inside WSL.

## Uninstall

```bash
rm ~/.local/bin/idx
```

Config and cache live in `~/.config/idx/` and `~/.cache/idx/`.

## Tests

`scripts/install-sh-test.sh` runs the script against a local fake release
server, so it needs no network. CI runs it with `sh` and `dash` on Linux and
with `sh` on macOS, plus ShellCheck.

```bash
scripts/install-sh-test.sh
INSTALL_SH_SHELL=dash scripts/install-sh-test.sh
```
