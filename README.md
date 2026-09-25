# idx-cli

CLI tool for Indonesian stock market (IDX) analysis, built in Rust for humans and AI agents.

## Installation

### Install script (Linux, macOS)

```bash
curl -fsSL https://raw.githubusercontent.com/0xrsydn/idx-cli/main/install.sh | sh
```

Downloads the prebuilt `idx` binary for your platform from GitHub Releases,
verifies it against the release `SHA256SUMS`, and installs it into
`~/.local/bin` without sudo. Pin a version or change the directory with
`sh -s -- --version v0.2.4 --dir ~/bin`. See [docs/INSTALL.md](docs/INSTALL.md).

### npm / npx

```bash
npx idx-cli --help          # run without installing
npm install --global idx-cli
```

The npm package downloads the same checksum-verified release binary during
`postinstall`. pnpm 10+ and bun skip install scripts by default; allow them
with `pnpm add --global --allow-build=idx-cli idx-cli` or
`bun add --global --trust idx-cli`. See
[docs/NPM_DISTRIBUTION.md](docs/NPM_DISTRIBUTION.md).

Prebuilt binaries cover Linux x64/arm64 (static musl) and macOS arm64/x64.
On Windows, use WSL.

### Nix

```bash
nix run github:0xrsydn/idx-cli -- version
nix profile install github:0xrsydn/idx-cli#default
```

The Nix package wraps `idx` with `curl-impersonate` and `mupdf`, so every
helper tool below is available automatically.

### Cargo

```bash
cargo install idx-cli
```

Builds from source; requires Rust `1.85+`.

### Runtime helper tools

The install script, npm, and Cargo installs ship only the `idx` binary. A few
commands call external tools:

| Tool | Needed for |
| --- | --- |
| `curl_chrome*` from [curl-impersonate](https://github.com/lexiforest/curl-impersonate) (ownership also tries its Firefox and Safari profiles), or `IDX_CURL_IMPERSONATE_BIN` | `ownership discover` / `import --url` from IDX (behind Cloudflare), and Yahoo fundamentals when `IDX_PROVIDER=yahoo` |
| `mutool` from MuPDF | importing legacy PDF ownership reports only |

Everyday use (`stocks` with the default MSN provider, including history,
`ownership sync`, and every local ownership query) needs neither.

## Quick start

```bash
# Stocks
idx stocks quote BBCA
idx stocks quote BBCA,BBRI,BMRI
idx stocks history BBCA --period 3mo
idx stocks technical BBCA
idx stocks fundamental BBCA
idx stocks compare BBCA BBRI BMRI
idx -o json stocks quote BBCA

# Ownership: download the maintained snapshot once, then query locally
idx ownership sync
idx ownership ticker BBCA
idx ownership entity "ANTHONI SALIM"
idx ownership search salim
idx ownership changes --from 2026-07-31 --to 2026-08-31
```

## Features

### Stocks

- Quotes, OHLC history, company profile, financial statements, earnings,
  news, crowd sentiment, and AI insights
- Analysis: `technical`, `growth`, `valuation`, `risk`, and a combined
  `fundamental` report; multi-symbol `compare`; MSN stock `screen`
- Providers: MSN is the default for quotes, fundamentals, and screening;
  Yahoo serves history. Switch with `IDX_PROVIDER=msn|yahoo` and
  `IDX_HISTORY_PROVIDER=auto|yahoo|msn`
- Local file cache with TTL, and `--offline` mode with stale-cache fallback

### Ownership (KSEI shareholders above 1%)

- Source: the monthly KSEI holder register ("Pemegang Saham di Atas 1%")
  published by IDX. Since June 2026 it is an XLSX file on IDX's
  [Data Kepemilikan Saham](https://www.idx.co.id/id/perusahaan-tercatat/data-kepemilikan-saham/)
  page; earlier months were PDF announcements
- `ownership sync` installs a maintained SQLite snapshot (checked daily,
  currently the latest three months), after which every query runs offline
- Queries: holders per `ticker`, holdings per `entity`, entity `search`,
  month-over-month `changes`, `concentration`, `cross-holders`, and an
  ownership `graph`
- Self-serve ingest: `ownership discover` finds the latest reports, and
  `ownership import --url <xlsx-or-pdf>` / `--file` loads them into your
  local database. See [docs/OWNERSHIP_SYNC.md](docs/OWNERSHIP_SYNC.md)

### Output

- Human-readable tables on stdout, JSON with `-o json`, errors on stderr

## Configuration

Config file location:

- `~/.config/idx/config.toml`

You can configure using:

- Config file (`idx config init`, `idx config set`)
- Environment variables
- CLI flags

Precedence order:

1. CLI flags
2. Environment variables
3. Config file
4. Built-in defaults

## Agent-friendly usage

- Use `idx --help` and subcommand help for discoverability
- Use `-o json` / `--output json` for structured output
- `idx completions <shell>` generates shell completions

## Documentation

- [docs/INSTALL.md](docs/INSTALL.md): install script options
- [docs/NPM_DISTRIBUTION.md](docs/NPM_DISTRIBUTION.md): npm package and release flow
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): providers, ownership ingest, and discovery
- [docs/OWNERSHIP_SYNC.md](docs/OWNERSHIP_SYNC.md): snapshot manifest and sync rules
- [docs/OWNERSHIP_PUBLISH.md](docs/OWNERSHIP_PUBLISH.md) and
  [docs/OWNERSHIP_SELF_HOSTED.md](docs/OWNERSHIP_SELF_HOSTED.md): maintainer snapshot publishing
- [docs/SMOKE.md](docs/SMOKE.md): smoke tests against live providers

## Development

```bash
nix develop
cargo test
```

Prek hooks are configured for quality gates:

- pre-commit: `cargo fmt --check` + `cargo clippy -- -D warnings`
- pre-push: `cargo test`

Script test suites (no live network): `scripts/install-sh-test.sh`,
`scripts/publish-ownership-snapshot-test.sh`, and `scripts/npm-smoke.sh`.

## License

MIT
