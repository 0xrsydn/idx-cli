# npm distribution

The npm package `idx-cli` is a small wrapper around the native `idx` binary.
The package keeps the JavaScript launcher and postinstall downloader. The
downloader selects a prebuilt GitHub Release asset for the current platform and
places it at the path used by the `idx` bin entry.

Supported release assets:

| npm platform | Rust target | Release asset |
| --- | --- | --- |
| Linux x64 | `x86_64-unknown-linux-musl` | `idx-linux-x64` |
| Linux arm64 | `aarch64-unknown-linux-musl` | `idx-linux-arm64` |
| macOS arm64 | `aarch64-apple-darwin` | `idx-darwin-arm64` |
| macOS x64 | `x86_64-apple-darwin` | `idx-darwin-x64` |

The Linux binaries are statically linked against musl, so they do not depend
on the host glibc version.

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

The postinstall script downloads `SHA256SUMS` from the same release and
refuses to install a binary whose checksum does not match.

### pnpm and bun

pnpm 10+ and bun do not run dependency lifecycle scripts by default, so the
binary is never downloaded and `idx` reports that it is missing. Allow the
postinstall script explicitly:

```bash
# pnpm
pnpm add --global --allow-build=idx-cli idx-cli

# bun
bun add --global --trust idx-cli
```

Plain `npm install` and `npx` run the postinstall script normally.

## Local smoke test

The smoke script builds or reuses `target/release/idx`, packs the npm wrapper,
installs the tarball into a new temporary directory, and runs the installed
`idx` command. It uses `IDX_BINARY_URL` with a local `file:` URL, so the test
does not need a registry publish or a network download.

```bash
nix develop -c bash -c 'scripts/npm-smoke.sh'
nix develop -c bash -c 'npm pack --dry-run'
```

The `postinstall` script performs the release download for normal package
installation. It skips the download when it runs inside this source checkout
(where `Cargo.toml` exists) unless `IDX_BINARY_URL` is set. `IDX_BINARY_URL`
accepts an HTTP(S) URL, a `file:` URL, or an existing local path for local
testing. Set `IDX_BINARY_SHA256` to verify an override binary as well.

## GitHub Release workflow

Pushing a tag that matches `v*` starts
`.github/workflows/release.yml`. The workflow builds all supported Rust
targets, names the binaries with the asset names above, writes `SHA256SUMS`,
and attaches everything to the matching GitHub Release. The asset names and
URLs must stay aligned with `scripts/install.js`.

To attach binaries to a tag that already exists, run the workflow manually:

```bash
gh workflow run release.yml -f tag=v0.2.3
```

Pull requests that touch the workflow or the installer run the build jobs
without publishing, so target breakage shows up before a release.

Release order:

1. Push the tag, or run the workflow for an existing tag.
2. Confirm the release has all binaries plus `SHA256SUMS`.
3. Run `npm publish`.
