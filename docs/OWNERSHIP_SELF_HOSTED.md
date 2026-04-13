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

## Manual Command

From a checked-out repo with GitHub auth already configured:

```bash
nix develop --command scripts/publish-ownership-snapshot.sh \
  --build \
  --idx-bin ./target/debug/idx \
  --output-dir /var/lib/idx/ownership-snapshot/current \
  --repo 0xrsydn/idx-cli \
  --release-tag ownership-snapshot-current
```

That helper:

1. optionally builds `idx`
2. discovers the latest supported IDX/KSEI PDF
3. imports it into an isolated temp DB
4. emits the SQLite snapshot and manifest
5. ensures the stable GitHub release exists
6. uploads the manifest and SQLite asset with `--clobber`

## systemd Service

Example service: [contrib/systemd/idx-ownership-snapshot-publish.service](/Users/rasyidanakbar/Development/myApp/idx-cli/contrib/systemd/idx-ownership-snapshot-publish.service)

Important assumptions:

- the repo checkout lives at `/srv/idx-cli`
- a writable publish directory exists at `/var/lib/idx-ownership-snapshot/current`
- `GH_TOKEN` is provided via an env file such as `/etc/idx-ownership-snapshot.env`
- the host can run `nix develop`

## systemd Timer

Example timer: [contrib/systemd/idx-ownership-snapshot-publish.timer](/Users/rasyidanakbar/Development/myApp/idx-cli/contrib/systemd/idx-ownership-snapshot-publish.timer)

The sample timer uses:

- `OnCalendar=*-*-02 09:00:00`
- `Persistent=true`
- `RandomizedDelaySec=30m`

That is intentionally conservative. The ownership source is monthly, but the
exact publish day can drift. Start with an early-month schedule and adjust after
observing a few real runs.

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
