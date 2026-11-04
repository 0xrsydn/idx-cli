# Changelog entries

Every pull request that changes what users or agents see adds one file here,
`changes/unreleased/<branch-name>.md`; for example, `fix/mcp-warnings` adds
`changes/unreleased/mcp-warnings.md`. Each PR has its own file, so stacked PRs
never conflict over the changelog. Internal refactors, CI-only and test-only
changes need no entry.

An entry holds `### <section>` headings with one bullet per change. Write each
bullet as one sentence for the user, saying what changed for them, and end it
with the PR number:

```markdown
### Bug Fixes

- `ownership import` reads IDX's register from October 2026 on, which adds a `CONTROLLER_OR_AFFILIATE` column. (#79)
```

The sections, in the order a release lists them:

| Section | For |
| --- | --- |
| `Security` | vulnerabilities, with what an attacker could do |
| `New Features` | new commands, tools, flags and providers |
| `Bug Fixes` | wrong output, crashes, failures |
| `Improvements` | speed, robustness, help text, distribution |
| `Output Changes` | any change to JSON fields, MCP results, error codes or exit codes that scripts and agents read; start a breaking one with **Breaking:** |
| `Docs` | user-facing documentation |
| `Testing` | what was verified, when a release needs to say so |
| `Notes` | dependencies, requirements, known limits |

A release may add `changes/unreleased/summary.md`: a short paragraph with no
headings that opens its notes, such as "**Upgrade from 0.3.2**: ...".

`scripts/changelog.sh check` (part of `nix flake check`) validates the entries.
At release time, `scripts/changelog.sh release X.Y.Z` folds them into a new
section of [`CHANGELOG.md`](../CHANGELOG.md) and deletes them; edit the section
if needed and commit it with the version bump. The release workflow publishes
that section as the GitHub release notes.
