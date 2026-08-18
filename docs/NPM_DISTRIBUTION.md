# npm distribution

The npm package `idx-cli` is a small wrapper around the native `idx` binary.
The package keeps the JavaScript launcher and postinstall downloader. The
downloader selects a prebuilt GitHub Release asset for the current platform and
places it at the path used by the `idx` bin entry.

Supported release assets:

| npm platform | Rust target | Release asset |
| --- | --- | --- |
| Linux x64 | `x86_64-unknown-linux-gnu` | `idx-linux-x64` |
| Linux arm64 | `aarch64-unknown-linux-gnu` | `idx-linux-arm64` |
| macOS arm64 | `aarch64-apple-darwin` | `idx-darwin-arm64` |

Windows and other platform combinations are out of scope. The postinstall
script reports the supported targets when it rejects a platform.

## Use the package

```bash
npm install --global idx-cli
idx --help

# Or run the package without a global install.
npx idx-cli --help
```

The package version and the release tag must match. For example, package
version `0.2.3` downloads assets from the `v0.2.3` GitHub Release.

## Local smoke test

The smoke script builds or reuses `target/release/idx`, packs the npm wrapper,
installs the tarball into a new temporary directory, and runs the installed
`idx` command. It uses `IDX_BINARY_URL` with a local `file:` URL, so the test
does not need a registry publish or a network download.

```bash
nix develop -c bash -c 'scripts/npm-smoke.sh'
nix develop -c bash -c 'npm pack --dry-run'
```

The `prepare` npm script builds the Rust release binary when npm packs the
wrapper from this checkout. The `postinstall` script performs the release
download for normal package installation. `IDX_BINARY_URL` accepts an HTTP(S)
URL, a `file:` URL, or an existing local path for local testing.

## GitHub Release workflow

Pushing a tag that matches `v*` starts
`.github/workflows/release.yml`. The workflow builds all three supported Rust
targets, names the binaries with the asset names above, and attaches them to
the matching GitHub Release. The asset names and URLs must stay aligned with
`scripts/install.js`.
