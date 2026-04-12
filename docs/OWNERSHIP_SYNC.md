# Ownership Snapshot Sync

`idx ownership sync` installs a maintained SQLite snapshot into the local ownership DB path.

The command is intentionally manifest-driven so the repo can publish snapshots in GitHub releases, object storage, or a local filesystem path without changing the CLI.

The recommended GitHub publish layout and maintainer workflow are documented in
`docs/OWNERSHIP_PUBLISH.md`.

## Consumer Inputs

The manifest location is resolved in this order:

1. `idx ownership sync --manifest <path-or-url>`
2. `IDX_OWNERSHIP_SNAPSHOT_MANIFEST`
3. `ownership.snapshot_manifest` in `config.toml`

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
    "download_url": "https://example.com/ownership-snapshot-2026-02-27.sqlite",
    "sqlite_sha256": "<64-char hex sha256>",
    "size_bytes": 123456,
    "release_count": 2,
    "latest_as_of_date": "2026-02-27",
    "latest_release_sha256": "<latest ownership_releases.sha256>",
    "latest_row_count": 7261,
    "ticker_count": 955
  }
}
```

Semantics:
- `source` is optional provenance metadata describing the IDX/KSEI PDF used to build the snapshot.
- `download_url` points to the SQLite artifact itself.
- `sqlite_sha256` and `size_bytes` are validated before install.
- `latest_*` and `release_count` are validated against the downloaded SQLite contents before replacement.

## Replacement Rules

Without `--force`, sync behaves conservatively:

- no local DB: install snapshot
- empty local DB: replace with snapshot
- local latest release older than snapshot: replace with snapshot
- local latest release/date/count matches snapshot: no-op
- local latest release matches but has fewer releases than snapshot: replace to fill missing history
- local DB newer than snapshot: no-op and keep local data
- local DB diverges at the same latest date: no-op and require `--force`

Replacement is staged through a temp file and validated before the local DB is swapped in.

## Publisher Workflow

Use `scripts/build-ownership-snapshot.sh` to copy a vetted ownership DB into a publishable artifact plus manifest:

```bash
scripts/build-ownership-snapshot.sh \
  --db /path/to/ownership.db \
  --output-dir dist/ownership-snapshot \
  --base-url https://example.com/idx/ownership
```

If `--base-url` is omitted, the generated manifest uses the local artifact path as `download_url`, which is useful for local testing.

For the maintainer flow that discovers the latest supported IDX/KSEI source
first and then builds GitHub-release-ready assets, use
`scripts/build-latest-ownership-snapshot.sh`.
