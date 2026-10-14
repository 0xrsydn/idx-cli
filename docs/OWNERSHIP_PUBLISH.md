# Ownership Snapshot Publishing

Maintainer workflow for producing and publishing the snapshot that
`idx ownership sync` consumes. The consumer contract (manifest schema and
replacement rules) is in `docs/OWNERSHIP_SYNC.md`.

## Release Layout

Snapshot assets live on a dedicated GitHub release, separate from versioned app
releases:

- tag: `ownership-snapshot-current`
- manifest: `ownership-snapshot-manifest.json` (stable URL, the commit point)
- SQLite: `ownership-snapshot-YYYY-MM-DD-<sha256>.sqlite` (immutable, content-addressed)

```text
https://github.com/0xrsydn/idx-cli/releases/download/ownership-snapshot-current/ownership-snapshot-manifest.json
```

Never use `.../releases/latest/download/...`: the repo's latest release is an
app release, not the snapshot.

## Running the Publisher

Packaged (recommended for hosts; no checkout or cargo build needed):

```bash
nix build github:0xrsydn/idx-cli#ownership-publisher
./result/bin/idx-ownership-publish --output-dir /var/lib/idx/ownership-snapshot/current --history 2
./result/bin/idx-ownership-freshness --max-age-days 40
```

From a checkout:

```bash
nix develop --command scripts/publish-ownership-snapshot.sh \
  --build \
  --idx-bin ./target/debug/idx \
  --output-dir dist/ownership-snapshot \
  --repo 0xrsydn/idx-cli \
  --release-tag ownership-snapshot-current
```

`idx-ownership-publish` wraps `scripts/publish-ownership-snapshot.sh` with a
pinned `idx` (including curl-impersonate and mupdf) plus jq, gh, sqlite, and
curl. It needs `GH_TOKEN` with permission to upload release assets; the freshness
check needs no token.

## What It Does

1. `idx -o json ownership discover --family above1 --limit 50` (newest supported
   report first; XLSX from the Data Kepemilikan Saham page since June 2026)
2. verifies the discovered report is the supported import path
3. imports it plus `--history <n>` earlier months, oldest first, into an
   isolated temp DB
4. checks the imported release metadata is non-empty and tied to the source URL
5. builds the SQLite artifact and manifest (`scripts/build-latest-ownership-snapshot.sh`,
   which calls `scripts/build-ownership-snapshot.sh`) and records `source`
   provenance in the manifest
6. uploads using the protocol below

## Publication Protocol

The manifest is the commit point, so uploads are ordered:

1. Upload the content-addressed SQLite asset. Same content means same name, so
   retries are idempotent.
2. Read the release assets back and verify the full SHA-256 and size (GitHub's
   `sha256:` digest when present, otherwise download and hash). Fail before
   touching the manifest if this does not match.
3. Upload the manifest last and verify its `snapshot.sqlite_sha256`.

A partial upload exits non-zero and is repaired by the next run; previously
published manifests stay valid because old SQLite assets are retained. GitHub
does not replace the manifest transactionally, so a failed replacement can leave
it unavailable until a retry succeeds.

The publisher treats the published snapshot as current only when the manifest's
`download_url` names this repo and tag and the referenced asset matches its
SHA-256 and size. A missing or corrupt asset, a mismatched URL, an unreadable
manifest, or any GitHub auth/network error means "publish", never "up to date".
`--force` uploads regardless. A forced rebuild for the already-published as-of
date adds the replaced build's `sqlite_sha256` to `replaces_sqlite_sha256`
(keeping the last 20), so clients still holding it update on their next sync.

## Run Result

The publisher is idempotent and prints exactly one final line:

| Last line | Exit | Meaning |
| --- | --- | --- |
| `RESULT: published <as-of>` | 0 | a newer snapshot, or larger history, was uploaded |
| `RESULT: up-to-date <as-of>` | 0 | nothing to upload |
| `RESULT: FAILED stage=<stage> (exit N)` | N | failed in `arguments`, `build`, `preflight`, `discover`, `build-snapshot`, `stage-output`, `upload`, or `publish` |

`idx-ownership-freshness` is the independent alarm: it exits 1 when the published
`latest_as_of_date` is older than `--max-age-days` (default 40). The source lands
2-3 days after month end, so 40 days only trips when publishing has stopped.

## History Coverage

`--history <n>` (maximum 1000) requests the latest report plus up to `n` earlier
monthly XLSX reports:

- desired releases = `1 + min(--history, available earlier XLSX months)`
- same as-of date with at least the desired release count: no-op
- same as-of date with fewer releases: republish to add history
- a request larger than the source's history is accepted once, not rebuilt forever

Each extra month adds about 2.4 MB, and history is what lets
`idx ownership changes` work straight from a synced snapshot.

## Snapshot Size Limit

idx clients up to v0.2.3 cap the snapshot download at **10 MiB** (10,485,760
bytes) and accept only uncompressed SQLite. A larger snapshot breaks
`idx ownership sync` for every existing install (this happened on 2026-09-25
with `--history 5`).

| `--history` | months | size (2026-08-31) |
| --- | --- | --- |
| 2 | 3 | 8,171,520 |
| 3 | 4 | 10,510,336 (too large) |
| 5 | 6 | 15,290,368 (too large) |

The builder refuses snapshots above `--max-snapshot-bytes` (default 10485760),
ending with `RESULT: FAILED stage=build-snapshot` and leaving the published
snapshot unchanged. Newer clients accept up to the manifest's `size_bytes` (at
most 1 GiB); raise both limits only once pre-v0.2.4 clients are rare.

## Automation

IDX has returned `403` to GitHub-hosted runners, so unattended publishing runs
on a self-hosted machine that IDX accepts:

- `contrib/systemd/idx-ownership-snapshot-publish.service` and `.timer` run the
  packaged publisher daily at 06:00 UTC with `GH_TOKEN` from
  `/etc/idx-ownership-snapshot.env`. Wire `OnFailure=` to a notification.
- On NixOS, add this repo as a flake input and use
  `inputs.idx-cli.packages.${system}.ownership-publisher`; `nix flake update idx-cli`
  picks up fixes.

`.github/workflows/publish-ownership-snapshot.yml` is a manual
(`workflow_dispatch`) fallback that runs the same packaged publisher with inputs
passed as environment variables.

Observed XLSX upload times (HTTP Last-Modified) were the 2nd-3rd of each month
around 02:00-04:00 UTC; a daily idempotent run makes the exact day irrelevant.
If the monthly source has not appeared yet, the run should fail loudly and retry
later rather than publish stale data.
