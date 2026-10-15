# idx-cli website: design

**Status:** Built 2026-10-15 (phases 1-5). The landing page is a standalone
Astro page (`src/pages/index.astro`); the docs are Starlight. The agent-smoke
case for the install prompt is still to do.
**Branch:** `feat/web`. **URL:** `https://idx.rasyidanaf.com`.

## Goals

1. Explain in one screen what idx does: Indonesian stock data and KSEI
   ownership, from a terminal or an AI agent.
2. Get someone from zero to a working `idx` or `idx-mcp` in under a minute,
   whatever their install path (npm, curl, Cargo, Nix) or agent.
3. Be readable by agents: every page as Markdown, plus `llms.txt`.
4. Stay correct as the CLI changes. Reference pages are generated from the
   tested sources, not written by hand.

Non-goals: a blog, a hosted API, live market data on the site, user accounts.

## Audiences

| Who | Arrives wanting | Lands on |
| --- | --- | --- |
| Developer / trader | "what is this, how do I install it" | landing, then Install |
| Agent user | "how do I add this to Claude Code / Codex / ..." | MCP setup |
| The agent itself | the output contract and the tool list | `llms.txt`, `/agents` |
| Contributor | architecture, tests | GitHub (`docs/`), not the site |

## Sitemap

```text
/                         landing
/start/install            npm, curl, Cargo, Nix; helper tools; Windows note
/start/quickstart         first stocks and ownership commands
/start/config             config file, env vars, precedence
/cli/stocks               guide: quote, history, analysis, screen, compare
/cli/ownership            guide: sync, ticker, entity, search, changes, graph,
                          self-serve import
/cli/output               table/json/jsonl, errors, exit codes, piping
/cli/reference            generated: every command and flag
/mcp                      what idx-mcp serves, envelope, limits
/mcp/setup                one tab per client (see below)
/mcp/tools                generated: the 11 tools (plus opt-in ownership_sync)
/agents                   using idx from an agent: contract, recipes, skill file
/agents/install.md        instructions an agent follows to install and register idx
/about                    data sources, freshness, disclaimer, license
/llms.txt, /llms-full.txt generated
```

Maintainer docs (`docs/ARCHITECTURE.md`, publishing, smoke tests) stay in the
repo and are linked, not copied.

## Landing page

```text
┌──────────────────────────────────────────────────────────────────────┐
│ idx                          Docs  MCP  Agents  GitHub ★   [search]  │
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  Indonesian stocks and ownership,         ┌── terminal ───────────┐  │
│  in your terminal and your agent.         │ $ idx stocks quote    │  │
│                                           │     BBCA BBRI BMRI    │  │
│  Quotes, fundamentals, technicals and     │ SYMBOL  PRICE  CHG%   │  │
│  KSEI shareholder data for IDX, as        │ BBCA    ...           │  │
│  tables or JSON. Ships an MCP server.     │                       │  │
│                                           │ $ idx ownership       │  │
│  [npm] [curl] [cargo] [nix]               │     ticker BBCA       │  │
│  ┌──────────────────────────────┐ [copy]  │ HOLDER          %     │  │
│  │ npm install -g idx-cli       │         │ ...                   │  │
│  └──────────────────────────────┘         └───────────────────────┘  │
│  [Get started]  [Add to your agent]                                  │
├──────────────────────────────────────────────────────────────────────┤
│  Stocks              Ownership                 Agents                │
│  quotes, history,    who holds >1% of every    JSON when piped,      │
│  fundamentals,       IDX stock, monthly        exit codes, MCP       │
│  technicals, screen  changes, offline queries  server with 11 tools  │
├──────────────────────────────────────────────────────────────────────┤
│  Add idx to your agent                                               │
│  [Claude Code] [Codex] [Cursor] [opencode] [pi] [Hermes] [other]     │
│  ┌──────────────────────────────────────────────────────┐ [copy]     │
│  │ claude mcp add idx -- npx -y idx-mcp                 │            │
│  └──────────────────────────────────────────────────────┘            │
│  "Who are BBCA's largest holders, and what changed last month?"      │
│   → get_shareholders, get_ownership_changes                          │
├──────────────────────────────────────────────────────────────────────┤
│  Ownership spotlight: one real ticker's holder table and a monthly   │
│  change, with the snapshot date shown.                               │
├──────────────────────────────────────────────────────────────────────┤
│  Built for pipes                                                     │
│  idx stocks screen --filter high-dividend | jq -r '.[].symbol' \     │
│    | idx stocks compare -                                            │
├──────────────────────────────────────────────────────────────────────┤
│  MIT · GitHub · npm · crates.io · Not affiliated with IDX or KSEI    │
└──────────────────────────────────────────────────────────────────────┘
```

On mobile the terminal moves below the hero text and the feature columns
stack.

## Visual direction

- **Mood:** a quiet terminal tool. Dark by default with a light theme,
  monospace for anything that's a command or output, generous whitespace,
  no stock-photo charts.
- **Colour:** neutral greys plus one accent. Proposal: the red of the
  Indonesian flag (`#CE1126`) for links and the primary button, with green
  and red only for price moves, matching the CLI's table colours.
- **Type:** system UI font for text, a monospace stack
  (`JetBrains Mono`, falling back to `ui-monospace`) for code. No web font
  is required for the first version.
- **Terminal panels:** static HTML rendered from real `idx` output, not
  images or GIFs, so they are copyable, searchable and readable by agents.
  Each panel shows the capture date.
- **Logo:** a wordmark, `idx` in monospace with a block cursor. No IDX or
  KSEI marks; the footer and `/about` say the project is unaffiliated.

## Content sources and drift

| Page | Source | How |
| --- | --- | --- |
| `/cli/reference` | `idx --help` per command | a script runs the built binary and writes Markdown |
| `/mcp/tools` | `crates/idx-mcp/tests/golden/tools.json` | rendered at build time from the golden |
| Terminal panels, BBCA table | real `idx` runs | copied by hand and labelled with their date; refresh after a new month |
| Install, guides | hand-written | reviewed when the README changes |

The README shrinks to a pitch, one install line per channel, and a link to the
site. The site becomes the one place for user docs.

## Agent features

- **Agent-led install.** The landing install tabs end with "Ask your agent",
  and the MCP setup page opens with the same box: a one-line prompt to paste
  into any coding agent:

  > Install idx-cli and its MCP server for me by following
  > https://idx.rasyidanaf.com/agents/install.md, then show me that it works.

  The prompt stays short and stable; the instructions live in
  `/agents/install.md`, served as plain Markdown. That file tells the agent
  to: detect OS and architecture; pick npm, the install script or Cargo
  (no sudo); confirm with `idx --version`; work out which harness it is
  running in and register `idx-mcp` with that harness's config from the
  setup tabs; ask before editing global config; run `idx ownership sync`;
  and finish with a test call (`get_shareholders BBCA`) reported back to the
  user. `tests/agents/agent-smoke.sh` gains a case that runs this prompt
  against each agent in a throwaway home, so the instructions stay working.

- `/llms.txt` (index) and `/llms-full.txt` (all docs in one file), from the
  `starlight-llms-txt` plugin.
- Every page also served as `<page>.md`, and a "Copy as Markdown" button.
- `/agents` covers the contract an agent relies on: JSON when piped, one
  error object on stderr, exit codes `2`-`5`, symbols from stdin, the MCP
  envelope and its limits.
- A downloadable skill file (`SKILL.md`) that teaches an agent to use the
  `idx` CLI directly, for harnesses where an MCP server isn't wanted.

## MCP setup tabs

Each tab gives one copyable command or config block, plus where the file
lives:

- **Claude Code:** `claude mcp add idx -- npx -y idx-mcp`.
- **Codex:** `~/.codex/config.toml`, `[mcp_servers.idx]`.
- **Cursor / VS Code:** `mcp.json`.
- **opencode:** `opencode.json`, `mcp.idx` with `type: "local"`.
- **pi:** `mcp.json`.
- **Hermes:** `config.yaml`, `mcp_servers.idx`.
- **Other:** the `idx-mcp` command over stdio.

The Claude Code, Codex, opencode, pi and Hermes configs are the ones `tests/agents/agent-smoke.sh` already runs, so the
site can link that script as proof they work.

## Stack

- **Astro + Starlight**, in `web/`, using bun (as the personal site does).
  Starlight brings search (Pagefind), sidebar, dark/light theme, i18n and
  MDX. The landing page is a custom Starlight `splash` page with components.
- **No client framework.** Tabs and copy buttons use Starlight's built-ins;
  the result is static HTML with almost no JavaScript.
- **Nix:** a `devShells.web` with bun, kept out of `nix flake check` so the
  Rust gate stays fast. The site has its own CI check (build plus link check).

## Hosting and deploy

- **Cloudflare Pages**, a new project `idx-cli-web`, connected to this
  repository through the GitHub integration (the personal site already uses
  it):
  - root dir `web`, build `bun install && bun run build`, output `dist`;
  - production branch `main`; previews for PRs;
  - build watch paths `web/**`, so Rust-only changes don't redeploy.
- **Domain:** add `idx.rasyidanaf.com` as a Pages custom domain. Cloudflare
  then creates the `idx` CNAME, which takes precedence over the zone's
  wildcard `*.rasyidanaf.com` record.
- **Generated references:** the CLI reference needs a built `idx`. Pages
  builds can't run Cargo cheaply, so the generated Markdown is committed, and
  a check in `nix flake check` fails when it is out of date (the same pattern
  as the goldens).

## Phases

1. **Scaffold:** Starlight in `web/`, theme and wordmark, deploy to a
   `*.pages.dev` preview.
2. **Landing + install + quickstart.**
3. **Guides:** stocks, ownership, output, config.
4. **MCP and agents:** setup tabs, generated tool reference, `/agents`,
   `llms.txt`, skill file.
5. **Generated CLI reference** and its drift check.
6. **Go live:** custom domain, README trimmed, links from npm and crates.io
   metadata (`homepage`).

## Open questions

1. Indonesian translation: English only at launch, or both from the start?
2. Accent colour: flag red, or something more neutral?
3. Terminal panels: real current data (dated), or stable sample data?
4. Analytics: none, or Cloudflare Web Analytics (cookieless)?
