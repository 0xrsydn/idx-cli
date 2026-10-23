---
title: Using idx from an agent
description: How an AI agent should call idx, through MCP or the CLI.
---

An agent can reach idx two ways:

- **MCP** ([setup](/mcp/setup/)): typed tools with input schemas, the best
  fit for chat-style agents.
- **The CLI**: for agents that run shell commands. No setup beyond
  installing `idx`; output is JSON automatically when captured.

## Using the CLI

What an agent needs to know:

- Output is JSON whenever stdout is not a terminal, so capture it and parse
  it; no flag is needed. Use `-o jsonl` for long lists.
- On failure stdout is empty and stderr holds one JSON object,
  `{"error": true, "code": "...", "message": "..."}`.
- Exit codes say what to do next: `2` fix the input, `3` not found (or no
  cached data offline), `4` provider unavailable or blocked, retry later, `5`
  unsupported, `1` other.
- Tickers are IDX codes (`BBCA`); `.JK` is optional. Several tickers can be
  passed at once to `stocks quote` and `stocks compare`.
- `stocks intraday` and `--provider stockbit` read Stockbit's public page,
  paced to one request a second across idx processes. Ask for the symbols
  you need rather than looping over the market; on `UPSTREAMBLOCKED`, stop
  instead of retrying.
- Ownership queries need `idx ownership sync` once; after that they run
  offline. `idx -o json ownership releases` shows which months exist.

## Recipes

```bash
# price and day change for several stocks
idx stocks quote BBCA BBRI BMRI | jq '.[] | {symbol, price, change_pct}'

# largest holders and concentration
idx ownership ticker BBCA | jq '{holders: [.holders[] | {name, pct: (.percentage_bps / 100)}], concentration}'

# what one investor holds
idx ownership entity "ANTHONI SALIM"

# holder changes between the two latest months
idx -o json ownership releases | jq -r '.[0:2][].as_of_date'
idx ownership changes --from 2026-08-31 --to 2026-09-30
```

Percentages in ownership output are basis points (`5494` = 54.94%).

## Skill file

For harnesses that load skills (Claude Code, pi, opencode, and others),
[`SKILL.md`](/skills/idx/SKILL.md) packages these rules so the agent uses the
CLI correctly without MCP. Save it as `idx/SKILL.md` in your harness's skills
directory, for example `~/.claude/skills/idx/SKILL.md`.

## Machine-readable docs

- [`/llms.txt`](/llms.txt): an index of these docs for language models.
- [`/llms-full.txt`](/llms-full.txt): every page in one file.
- Any page as Markdown: add `.md` to its path, for example
  [`/agents/install.md`](/agents/install.md).
