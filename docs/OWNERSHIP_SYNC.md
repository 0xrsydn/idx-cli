# Ownership Snapshot Sync

`idx ownership sync` installs a maintained SQLite snapshot into the local ownership DB path.

The command is intentionally manifest-driven so the repo can publish snapshots in GitHub releases, object storage, or a local filesystem path without changing the CLI.

Publishing snapshots is a maintainer task; see `docs/OWNERSHIP_PUBLISH.md`.

## Consumer Inputs

The manifest location is resolved in this order:

1. `idx ownership sync --manifest <path-or-url>`
2. `IDX_OWNERSHIP_SNAPSHOT_MANIFEST`
3. `ownership.snapshot_manifest` in `config.toml`
4. built-in published manifest URL:
   `https://github.com/0xrsydn/idx-cli/releases/download/ownership-snapshot-current/ownership-snapshot-manifest.json`

The value can be either:
- a local path to a manifest JSON file
- a remote `http://` or `https://` URL

## Manifest Contract

Current schema version: `1`

```json
{
  "schema_version": 1,
  "generated_at": "2026-03-31T12:00:00Z",
  "source": {
    "family": "above1",
    "listing_page_url": "https://www.idx.co.id/id/berita/pengumuman/",
    "query_url": "https://www.idx.co.id/primary/NewsAnnouncement/GetAllAnnouncement?...",
    "pdf_url": "https://www.idx.co.id/StaticData/NewsAndAnnouncement/...pdf",
    "title": "Pemegang Saham di atas 1% (KSEI)",
    "publish_date": "2026-03-10T00:00:00",
    "original_filename": "b9b638e5a8_8928aca255.pdf"
  },
  "snapshot": {
    "kind": "sqlite",
    "compression": "none",
    "version": "2026-02-27",
    "download_url": "https://example.com/ownership-snapshot-2026-02-27-<sha256>.sqlite",
    "sqlite_sha256": "<64-char hex sha256>",
    "size_bytes": 123456,
    "release_count": 2,
    "latest_as_of_date": "2026-02-27",
    "latest_release_sha256": "<latest ownership_releases.sha256>",
    "latest_row_count": 7261,
    "ticker_count": 955,
    "replaces_sqlite_sha256": ["<sqlite_sha256 of an earlier build for this date>"]
  }
}
```

Semantics:
- `source` is optional provenance metadata describing the IDX/KSEI report used to build the snapshot.
  `source.pdf_url` is a legacy field name: since June 2026 it holds the XLSX URL.
- `download_url` points to the SQLite artifact itself. The filename is an
  implementation detail: published assets are immutable and content-addressed
  (`ownership-snapshot-YYYY-MM-DD-<sha256>.sqlite`), so consumers must always
  follow `download_url` and validate it with `sqlite_sha256` and `size_bytes`.
- `sqlite_sha256` and `size_bytes` are validated before install.
- `latest_*` and `release_count` are validated against the downloaded SQLite contents before replacement, including the number of holdings rows actually stored for the latest release.
- A manifest fetched over `http(s)://` must give an `http(s)://` `download_url`; only a local manifest may point at a local file. Local files must be regular files, and manifests are limited to 1 MiB.
- `replaces_sqlite_sha256` (optional, omitted when empty) lists earlier builds
  published for the same `latest_as_of_date` that this snapshot supersedes, for
  example after a parser fix. Clients up to v0.2.x ignore it.

## Replacement Rules

Without `--force`, sync behaves conservatively:

- no local DB: install snapshot
- empty local DB: replace with snapshot
- local latest release older than snapshot: replace with snapshot
- local DB file is byte-identical to `sqlite_sha256`: no-op
- local DB file is byte-identical to a build in `replaces_sqlite_sha256`: replace
  (queries never write to a synced file, so this means it was not modified;
  committed changes still in the `-wal` file are checkpointed before hashing)
- local latest release/date/count matches snapshot: no-op (a same-date build
  with a different row count replaces the local file only when it lists that
  file in `replaces_sqlite_sha256`)
- local latest release matches but has fewer releases than snapshot: replace to fill missing history
- local DB newer than snapshot: no-op and keep local data
- local DB diverges at the same latest date: no-op and require `--force`

Replacement is staged through a temp file and validated before the local DB is swapped in with a single rename, so an interrupted sync leaves either the old database or the new one.
