---
name: delete-comment
description: Remove code comments from idx-cli's Rust sources and ratchet the comment baseline down. Use when asked to "delete comments", "run /delete-comment", clean up comments in a file or crate, or when the comment lint (crates/idx-cli/tests/comments.rs) fails. Takes an optional scope (a file, directory, or crate); default is the files with the highest counts in the baseline.
---

# /delete-comment

idx-cli bans comments in Rust sources: the code has to explain itself, and
`docs/` holds architecture. `crates/idx-cli/tests/comments.rs` counts the
comment lines in each file and checks them against
`crates/idx-cli/tests/comment_baseline.txt`. A file may not go above its
count, and a lower count must be written back. This skill brings the counts
down.

Read `docs/CONVENTIONS.md` ("Comments" and "Testing") before you start.

## 1. Pick the scope

- If the user named a scope, use it.
- Otherwise list the largest counts:
  `sort -rn crates/idx-cli/tests/comment_baseline.txt | head`.
  Take one module per run so the diff stays reviewable.

What counts: `//`, `/* */`, `//!` and `///`. The exception is a doc comment
on an item that derives clap's `Parser`/`Args`/`Subcommand`/`ValueEnum` or
schemars' `JsonSchema`. Those become `--help` text, the website's CLI
reference, or MCP tool schemas, so they are data. Leave them alone; editing
them changes golden output.

## 2. Classify every comment in scope

| Category | What it is | Action |
| --- | --- | --- |
| Tautology | Restates what the code already shows (`// Truncate long descriptions` above `truncate_chars(...)`), section banners, `/// Returns the quote` on `fn quote()` | Delete. |
| Stale | Describes code, a bug, an audit or a history that is gone ("used to rate healthy", "the round-12 audit") | Delete. |
| Excuse | Justifies a hack instead of fixing it ("not recorded yet", "kept for future use", blanket `#[allow(...)]` with a reason) | Delete the comment, keep the code, and file an issue (step 4). |
| Decision record | A non-obvious rule, invariant, upstream quirk or tradeoff | Move it into code (step 3), then delete it. |

Be strict about Tautology. Before deleting a comment as obvious, ask whether
someone could undo the rule without noticing. If they could, it is a Decision
record.

## 3. Move each decision record into code

Use the first of these that fits:

1. **A test that fails when the rule breaks.** If one exists, the comment can
   go; put the reason in that test's assertion message
   (`assert_eq!(x, y, "a quadrillion rupiah, far past any session, is still exact")`)
   or in its name. If none exists and the rule matters, write the test. The
   test is the record.
2. **A name.** Extract a function (`write_whole_or_not_at_all`,
   `safe_in_cache_file_names_and_urls`), a constant (`OP_PING`,
   `SHORTEST_NAME_THAT_IS_NOT_A_FRAGMENT`,
   `MSN_PUBLIC_FRONTEND_API_KEY`), or rename a variable
   (`fetched_before_ranking`).
3. **A type**, so the wrong state cannot be written.
4. **A user-facing message**: an error or warning that already says why.
5. **`docs/`**, only for cross-cutting architecture: one rule repeated across
   modules, or a design boundary. Add a line to the right section of
   `docs/ARCHITECTURE.md`, `DESIGN.md` or `STOCKBIT.md`. Check first whether
   it is already there; then the comment is a duplicate and can go.

In tests, a comment above a case list becomes labelled tuples:
`("source url not http(s)", args)`, with the label used in the assertion
message.

If none of these fits yet, for example because the rule needs a concurrency
test you can't write in this pass, leave the comment. The ratchet allows it.
Record it as "kept" in the ledger with what it would need.

Do not change behavior. Do not edit golden files or fixtures.

## 4. File issues for excuses

One issue per hack, with `gh issue create`. Give it the file:line, what the
hack is, why it is a problem, and what fixing it means. Don't leave a
`TODO`/`FIXME` in the code; the issue is the record.

## 5. Ratchet and verify

```bash
IDX_UPDATE_COMMENT_BASELINE=1 nix develop --command cargo test -p idx-cli --test comments
git diff crates/idx-cli/tests/comment_baseline.txt   # counts must only go down
nix flake check -L
```

If you touched provider code, also run a few real commands against live
upstreams (`idx stocks quote BBCA`, `idx stocks news BBCA`, ...) and report
the outcome, including IDX `403`s.

## 6. Report

Give one ledger line per comment: `path:line|Category|action|note`, where
action is `deleted`, `assert-msg`, `renamed`, `extracted`, `docs`,
`test-added`, `issue` or `kept`. Add the before and after totals from the
baseline and the issue links. Put the ledger in the PR description.
