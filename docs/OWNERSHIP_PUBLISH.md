# Ownership Snapshot Publishing

This document covers the maintainer workflow for producing and publishing the
ownership snapshot artifacts consumed by `idx ownership sync`.

## Goals

- Build the snapshot from the currently discoverable supported IDX/KSEI source,
  not from an ad hoc local DB.
- Keep the published manifest URL stable for end users.
- Record source provenance in the manifest so the published SQLite artifact can
  be traced back to the live IDX announcement and PDF URL used to build it.

## Recommended GitHub Releases Layout

Use a dedicated stable release tag for snapshot assets:

- tag: `ownership-snapshot-current`
- manifest asset: `ownership-snapshot-manifest.json`
- SQLite asset: `ownership-snapshot-YYYY-MM-DD-<sha256>.sqlite` (immutable, content-addressed)

Recommended public manifest URL:

```text
https://github.com/0xrsydn/idx-cli/releases/download/ownership-snapshot-current/ownership-snapshot-manifest.json
```

Avoid `.../releases/latest/download/...` if normal app releases and snapshot
publishes share the same repository. The repo's "latest" release can drift away
from the ownership snapshot release.

## Manual Maintainer Flow

Run the publisher helper inside `nix develop` so `mutool` and the
`curl-impersonate` helper are available:

```bash
nix develop --command cargo build
nix develop --command scripts/publish-ownership-snapshot.sh \
  --idx-bin ./target/debug/idx \
  --output-dir dist/ownership-snapshot \
  --repo 0xrsydn/idx-cli \
  --release-tag ownership-snapshot-current
```

The helper performs these steps:

1. `idx -o json ownership discover --family above1 --limit 50` (newest supported
   report first; XLSX from the Data Kepemilikan Saham page since June 2026)
2. verifies the discovered report is the current supported import path
3. imports that report (plus `--history <n>` earlier months, oldest first) into
   an isolated temp ownership DB
4. checks that the imported release metadata is non-empty and tied to the same
   source URL
5. runs `scripts/build-ownership-snapshot.sh` to emit the SQLite artifact and
   base manifest
6. enriches the manifest with `source` provenance metadata

The resulting manifest records:

- `source.family`
- `source.listing_page_url`
- `source.query_url`
- `source.pdf_url`
- `source.title`
- `source.publish_date`
- `source.original_filename`

That metadata is additive. Existing sync clients can still parse the manifest.

## Publication Protocol

The manifest is the commit point that `idx ownership sync` follows. The
publisher therefore never uploads the manifest and the SQLite asset in one
non-atomic call. It uses this order:

1. Build the snapshot and stage the manifest plus one content-addressed SQLite
   file.
2. Upload the SQLite asset first. The name includes the artifact's SHA-256, so
   the same content always has the same name and a retry is idempotent.
3. Read the release assets back and confirm the SQLite asset exists with the
   expected SHA-256 and size. Use GitHub's `sha256:` digest metadata when it is
   present; for legacy assets without a digest, download the asset through `gh`
   and hash it. Fail before touching the manifest if verification fails.
4. Upload the manifest last and verify that its `snapshot.sqlite_sha256`
   matches the staged manifest.

Consequences:

- A partial upload (manifest uploaded, SQLite missing) can no longer happen.
- If either upload fails, the job exits non-zero and the next run retries.
  Previously downloaded manifests remain valid because the old SQLite asset is retained.
- GitHub does not replace the manifest transactionally. A failed replacement
  can leave the manifest unavailable until a retry succeeds.
- The publisher treats a published manifest as current only when `download_url`
  names this repo and release tag and the referenced asset matches the manifest
  SHA-256 and size. A missing or corrupt asset, a mismatched URL, or any GitHub
  authentication/network error means "publish", never "up to date".

## History Coverage

`--history <n>` requests the latest report plus up to `<n>` earlier monthly
XLSX reports. The publisher compares the request with the months the source
actually exposes:

- desired releases = `1 + min(--history, available earlier XLSX months)`
- a published snapshot with the same as-of date and at least the desired number
  of releases is a no-op
- a published snapshot with the same as-of date but fewer releases is
  republished to add history
- when the source has fewer historical months than requested, the publisher
  accepts the smaller coverage and does not rebuild on every run

## Upload Step

Prefer the publisher helper, which implements the protocol above:

```bash
scripts/publish-ownership-snapshot.sh \
  --idx-bin ./target/debug/idx \
  --output-dir dist/ownership-snapshot \
  --repo 0xrsydn/idx-cli \
  --release-tag ownership-snapshot-current
```

If you upload by hand, upload these files to the `ownership-snapshot-current`
GitHub release in this order:

1. `dist/ownership-snapshot/ownership-snapshot-YYYY-MM-DD-<sha256>.sqlite`
2. `dist/ownership-snapshot/ownership-snapshot-manifest.json`

Never upload the manifest before its SQLite asset is present. Only after this
manual flow is reliable should the repo automate it in GitHub Actions.

## GitHub Actions Workflow

The repo now includes a manual workflow at
`.github/workflows/publish-ownership-snapshot.yml`.

Current behavior:

- trigger: `workflow_dispatch` only
- builds the packaged publisher with `nix build .#ownership-publisher`
- runs `idx-ownership-publish` from that package, which performs discovery,
  import, snapshot build, and the full upload protocol in one pass
- uploads any locally produced artifacts as a workflow artifact copy with
  `if-no-files-found: ignore`, so a legitimate up-to-date run still succeeds
- does not run a separate release-create or asset-upload step

This is intentionally manual-first. Add a schedule only after a few successful
publish runs confirm the live source remains stable enough.

The workflow passes its inputs to the shell through environment variables and
routes publication through the same fixed publisher used by self-hosted hosts,
so it cannot drift from the CLI protocol.

## Self-Hosted Automation

GitHub-hosted Actions are still not sufficient for this job on their own. Live
verification has shown IDX returning `403` to GitHub-hosted runners during
discovery/import.

The recommended unattended path is therefore:

- keep the public artifacts on GitHub Releases
- run the publish job from a self-hosted machine that IDX accepts
- use a `systemd` oneshot service plus `systemd.timer` on that machine

See `docs/OWNERSHIP_SELF_HOSTED.md` for the reusable helper script, sample
systemd units, and the recommended split between this repo and `clan-private`.
