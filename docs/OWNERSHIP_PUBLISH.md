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
- SQLite asset: `ownership-snapshot-YYYY-MM-DD.sqlite`

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
nix develop --command scripts/build-latest-ownership-snapshot.sh \
  --idx-bin ./target/debug/idx \
  --output-dir dist/ownership-snapshot \
  --repo 0xrsydn/idx-cli \
  --release-tag ownership-snapshot-current
```

The script performs these steps:

1. `idx -o json ownership discover --family above1 --limit 1`
2. verifies the discovered report is the current supported import path
3. imports that PDF into an isolated temp ownership DB
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

## Upload Step

After the local build succeeds, upload these two files to the
`ownership-snapshot-current` GitHub release:

- `dist/ownership-snapshot/ownership-snapshot-manifest.json`
- `dist/ownership-snapshot/ownership-snapshot-YYYY-MM-DD.sqlite`

Only after this manual flow is reliable should the repo automate it in
GitHub Actions.

## GitHub Actions Workflow

The repo now includes a manual workflow at
`.github/workflows/publish-ownership-snapshot.yml`.

Current behavior:

- trigger: `workflow_dispatch` only
- builds `idx` inside `nix develop`
- runs `scripts/build-latest-ownership-snapshot.sh`
- uploads the generated files as workflow artifacts
- creates the stable release tag if needed
- uploads the manifest and SQLite asset to that release with `--clobber`

This is intentionally manual-first. Add a schedule only after a few successful
publish runs confirm the live source remains stable enough.

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
