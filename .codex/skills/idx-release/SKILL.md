---
name: idx-release
description: Prepare, publish, and repair versioned idx-cli application releases across Cargo.toml, Git tags, GitHub Releases, and crates.io. Use when Codex needs to cut a new app release, backfill a missing GitHub release, reconcile mismatched tags and crates.io versions, or verify release hygiene. Do not use this skill for the monthly ownership snapshot artifact flow under ownership-snapshot-current.
---

# IDX Release

## Overview

Handle versioned `idx-cli` application releases. Keep the package version, release
commit, Git tag, GitHub Release, and crates.io publication aligned. Treat the
ownership snapshot artifacts as a separate release track.

## Read First

Read these files before mutating release state:

- `Cargo.toml` for the package version
- `TODO.md` for the latest smoke findings and release-readiness notes
- `README.md` if install or publish notes may need updates
- `docs/OWNERSHIP_SYNC.md` and `docs/OWNERSHIP_PUBLISH.md` only when checking
  ownership-sync behavior or keeping snapshot and app releases separated

## Keep Release Tracks Separate

Treat these as different things:

- Versioned app releases: `v0.1.0`, `v0.2.1`, `v0.2.2`
- Snapshot artifact release: `ownership-snapshot-current`

Do not compare `ownership-snapshot-current` against crates.io, and do not let it
drive app-release hygiene decisions. Avoid `.../releases/latest/download/...` for
ownership snapshot docs or manifests because GitHub's latest app release can move.

## Determine The Source Commit First

Start by identifying the exact commit that corresponds to the target version.

- If crates.io already has the version, find the historical commit where
  `Cargo.toml` was bumped or published.
- If the Git tag already exists, do not move it.
- If the GitHub Release object is missing but the tag exists, backfill the
  release object only.
- If crates.io has a version that GitHub does not, create the missing tag and
  release at the historical commit, not at current `HEAD`.

Use commands like:

```bash
rg '^version\\s*=\\s*"' Cargo.toml
git log --oneline --decorate -- Cargo.toml
git blame -L 1,20 Cargo.toml
gh release list --repo 0xrsydn/idx-cli --limit 20
gh api 'repos/0xrsydn/idx-cli/tags?per_page=20'
curl -fsSL https://crates.io/api/v1/crates/idx-cli
```

## Verify Before Publishing

For a real release or release-hygiene fix, verify the repo state first.

Run the standard checks inside `nix develop`:

```bash
nix develop --command cargo build
nix develop --command cargo clippy -- -D warnings
nix develop --command cargo test
nix develop --command cargo package --locked
```

Run targeted smoke when the release changes user-facing CLI behavior. For
ownership-sync changes, re-verify a clean-machine flow:

```bash
env XDG_DATA_HOME=/tmp/idx-release-check/data \
    XDG_CACHE_HOME=/tmp/idx-release-check/cache \
    XDG_CONFIG_HOME=/tmp/idx-release-check/config \
    ./target/debug/idx ownership sync
```

If the user only asks for inspection or planning, stop after verification.

## Publish Or Repair GitHub Release State

Use `gh` for remote release hygiene.

For backfilled historical releases:

- use `gh release create <tag> --verify-tag --latest=false ...` when the tag
  already exists and only the release object is missing
- use `gh release create <tag> --target <sha> --latest=false ...` when both the
  tag and release are missing for an older version

For the current stable app release:

- use `gh release create <tag> --target <sha> --latest ...` when that version
  should become the latest GitHub app release

After creating or repairing a release, verify:

```bash
gh api repos/0xrsydn/idx-cli/releases/latest
gh release list --repo 0xrsydn/idx-cli --limit 20
gh api 'repos/0xrsydn/idx-cli/tags?per_page=20'
```

## Publish To crates.io Carefully

Treat `cargo publish` as irreversible.

- Do not publish to crates.io unless the user clearly asked to ship a new
  version.
- Do not publish a version that already exists on crates.io.
- Do not publish from a dirty tree unless the user explicitly accepts that risk.

Before publish, verify the version in `Cargo.toml`, confirm the intended commit,
and make sure the GitHub tag/release plan matches that exact version.

## Write Release Notes With Scope Discipline

Keep notes aligned to the actual tagged commit.

- Do not describe features that landed after the tagged commit.
- If backfilling a release object, say that clearly in the notes.
- If a release only fixes parser/import behavior, do not present later snapshot
  automation work as part of that version.
- Mention ownership-sync behavior only if that version actually included it.

## Final Verification

Before concluding, report:

- the version on crates.io
- the Git tag and commit SHA
- the GitHub Release URL
- whether GitHub `latest` now points at the expected app release
- whether any snapshot-artifact release state remains intentionally separate
