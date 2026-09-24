# Self-Hosted Ownership Snapshot Publish

This document covers the recommended self-hosted automation path for publishing
the monthly ownership snapshot consumed by `idx ownership sync`.

Use this path when:

- the repo's public release assets should stay on GitHub Releases
- IDX blocks GitHub-hosted Actions with `403`
- you have a VPS or other trusted machine that can reach the IDX source

## Why Self-Hosted

The current GitHub-hosted workflow remains useful as a manual reference, but it
is not reliable enough for unattended publishing because IDX has returned `403`
to GitHub-hosted runners. The self-hosted path keeps the end-user experience
simple:

- maintainers publish from a trusted machine
- end users still run plain `idx ownership sync`

No Nix setup is required for end users. Nix is only an implementation detail for
the maintainer host if that host already uses NixOS.

## Packaged Publisher (recommended)

The flake exposes a self-contained publisher, so the host needs no checkout,
`nix develop`, or cargo build at run time:

```bash
nix build github:0xrsydn/idx-cli#ownership-publisher
./result/bin/idx-ownership-publish --output-dir /var/lib/idx/ownership-snapshot/current --history 5
./result/bin/idx-ownership-freshness --max-age-days 40
```

`idx-ownership-publish` wraps `scripts/publish-ownership-snapshot.sh` with a
pinned `idx` (itself wrapped with curl-impersonate and mupdf) plus jq, gh,
sqlite and curl. On NixOS, add this repo as a flake input and run
`inputs.idx-cli.packages.${system}.ownership-publisher`; bump the input
(`nix flake update idx-cli`) to pick up fixes.

## Run Semantics

The publisher is idempotent and prints exactly one final line:

| Last line | Exit | Meaning |
| --- | --- | --- |
| `RESULT: published <as-of>` | 0 | a newer snapshot was uploaded |
| `RESULT: up-to-date <as-of>` | 0 | the published snapshot already has the latest report; nothing uploaded |
| `RESULT: FAILED stage=<stage> (exit N)` | N | failed in `check-published`, `discover`, `build-snapshot`, `stage-output`, or `upload` |

It checks the published manifest first and, for XLSX sources, compares as-of
dates before downloading anything, so it is cheap to run **daily**. `--force`
uploads regardless.

`idx-ownership-freshness` is the independent alarm: it exits 1 when the
published `latest_as_of_date` is older than `--max-age-days` (default 40). The
source is monthly and lands 2-3 days after month end, so 40 days only trips
when publishing has actually stopped working. Wire both to an `OnFailure=`
notification.

## Manual Command (from a checkout)

```bash
nix develop --command scripts/publish-ownership-snapshot.sh \
  --build \
  --idx-bin ./target/debug/idx \
  --output-dir /var/lib/idx/ownership-snapshot/current \
  --repo 0xrsydn/idx-cli \
  --release-tag ownership-snapshot-current
```

## systemd Units

Examples: `contrib/systemd/idx-ownership-snapshot-publish.service` and
`contrib/systemd/idx-ownership-snapshot-publish.timer` (daily at 06:00 UTC).

Observed XLSX upload times (HTTP Last-Modified): 2026-06-03,
2026-07-02 03:55Z, 2026-08-02 01:50Z, 2026-09-02 03:51Z. With a daily,
idempotent run the exact day no longer matters.

Pass `--history <n>` to include earlier months so `idx ownership changes`
works straight from the synced snapshot (about 2.4 MB per extra month).

## Clan Integration Later

The clean split for `clan-private` is:

1. keep the workflow logic in this repo
2. keep host-specific secrets and enablement in `clan-private`

Recommended integration shape on `greencloud-vps`:

- store a checked-out copy of `idx-cli` on the host, for example at `/srv/idx-cli`
- provide `GH_TOKEN` through Clan vars or another secret mechanism
- add a small NixOS module that installs/enables the service and timer
- set the module's `WorkingDirectory`, env file path, and publish directory

The existing pattern in `clan-private/modules/workspace-backup.nix` is a good
fit: a oneshot `systemd` service plus a `systemd.timer`.

## Operational Notes

- Keep `ownership-snapshot-current` separate from versioned app releases.
- Prefer manual runs first, then enable the timer after a few successful
  publishes.
- If the monthly source has not appeared yet, the service should fail loudly and
  be retried later; do not silently publish stale assumptions.
