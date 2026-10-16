# npm distribution

The npm packages `idx-cli` (repo root) and `idx-mcp` (`npm/idx-mcp/`) are small
wrappers around the native `idx` and `idx-mcp` binaries. Both use the same
launcher and installer from `scripts/`; the package's `nativeBinary` field
picks the binary, and `npm/idx-mcp` copies the scripts in at `prepack`.
The `idx-cli` package
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

`idx-mcp` assets use the same platforms with an `idx-mcp-` prefix, for
example `idx-mcp-linux-x64`.

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
version `0.3.1` downloads assets from the `v0.3.1` GitHub Release.

The postinstall script downloads `SHA256SUMS` from the same release and
refuses to install a binary whose checksum does not match.

### When postinstall does not run

npm 12 blocks install scripts of unapproved packages under `npx` and
`npm install -g` (its `allow-scripts` policy), and pnpm 10+ and bun skip
dependency lifecycle scripts by default. The launcher then runs the installer
itself on first start, logging to stderr so the `idx-mcp` stdout channel stays
clean; it fails only when that download fails too. Hermes Agent runs `npx` with
its own npm 12, which is how 0.3.1 broke there (issue #41).

The binary goes in the package's `bin/`, or, when that is not writable (a
root-owned `sudo npm install -g` whose postinstall npm 12 skipped), in a
per-user cache: `$XDG_CACHE_HOME`, else `~/.cache` (`~/Library/Caches` on
macOS), under `<package>/<version>/`. The launcher looks in both. A
termination signal during that first-run download stops the installer too,
and it removes its partial file.

To install the binary up front instead, allow the script explicitly:

```bash
# pnpm
pnpm add --global --allow-build=idx-cli idx-cli

# bun
bun add --global --trust idx-cli

# npm 12
npm install --global --allow-scripts=idx-cli idx-cli
```

## Local smoke test

The smoke script builds or reuses `target/release/idx`, packs the npm wrapper,
installs the tarball into a new temporary directory, and runs the installed
`idx` command, then installs `idx-mcp` again with `--ignore-scripts` to check
the launcher's first-run install, also with a read-only package directory
(the per-user cache fallback). It uses `IDX_BINARY_URL` with a local `file:` URL, so the test
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
gh workflow run release.yml -f tag=v0.3.1
```

Pull requests that touch the workflow or the installer run the build jobs
without publishing, so target breakage shows up before a release.

Release order:

1. Bump the version together in `Cargo.toml` (workspace version and the
   `idx-core` dependency pin), `package.json`, and `npm/idx-mcp/package.json`.
   A version number can never be republished.
2. Push the tag, or run the workflow for an existing tag.
3. Publish the crates from the tagged commit, dependency first:
   `cargo publish -p idx-core`, then `-p idx-cli` and `-p idx-mcp`.
4. Confirm the release has all eight binaries plus `SHA256SUMS`.
5. Run `npm publish` in the repo root and in `npm/idx-mcp/` from the tagged
   commit. The account has two-factor auth, so npm asks for a browser
   approval or `--otp=<code>`.
