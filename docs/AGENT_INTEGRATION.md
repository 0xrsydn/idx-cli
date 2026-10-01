# Agent Integration Roadmap

**Status:** Planning — prerequisites are the remaining P0/P1 core gaps in `FEATURE_SPEC.md`
**Goal:** Make `idx-cli` a first-class tool for AI agent workflows (Claude Code, OpenClaw, MCP clients)

---

## Current Foundation

The CLI already has the building blocks:

- Structured JSON output (`-o json`) on every command
- Trait-based provider abstraction (mockable, extensible)
- Rich typed domain models (`Quote`, `Fundamentals`, `CompanyProfile`, `OwnershipHolder`, etc.)
- Structured error codes in JSON mode
- File-based cache with TTL, offline, and stale-cache fallback
- Feature-gated modules (ownership behind `--features ownership`)

---

## Tier 1 — MCP Server Mode (highest leverage)

Add `idx mcp-serve` to expose existing commands as MCP tools over stdio.

- Use `rust-mcp-sdk` crate for the stdio JSON-RPC transport
- Each existing command maps to one MCP tool (e.g. `get_stock_quote`, `get_financials`, `screen_stocks`, `get_ownership_ticker`)
- Existing JSON serialization becomes the tool response body — minimal new code
- Tool input schemas derive from clap structs or parallel types
- Configure in Claude Code:
  ```json
  { "mcpServers": { "idx": { "command": "idx", "args": ["mcp-serve"] } } }
  ```

**Why this first:** One feature makes `idx-cli` instantly usable from Claude Code, Claude Desktop, OpenClaw, and any MCP-compatible client. No wrapper scripts needed.

**Reference:** `financial-datasets/mcp-server` (1.7k stars), `tradingview-mcp` (500 stars), `twsemcp` (Taiwan Stock Exchange MCP — closest regional analog), `rust-mcp-sdk` (Rust MCP crate).

---

## Tier 2 — Composability

### Stdin symbol input

```bash
idx stocks screen --filter pe_lt:15 -o json | jq -r '.[].symbol' | idx stocks fundamental --stdin -o json
```

Agents generate symbol lists dynamically. Piping enables chained workflows without temp files.

### JSONL output

```bash
idx -o jsonl stocks quote BBCA BBRI
```

Newline-delimited JSON — better for streaming and piping than pretty-printed arrays.

### Batch mode

```bash
idx batch commands.jsonl
```

Where each line is a command spec:
```json
{"command": "stocks/quote", "symbols": ["BBCA", "BBRI"]}
{"command": "ownership/ticker", "symbol": "BBCA", "source": "ksei"}
```

Reduces process spawn overhead when agents run many queries in sequence.

---

## Tier 3 — Schema Export

```bash
idx schema stocks/quote
```

Dumps the JSON Schema for a command's output. Agents use this to understand available fields without trial-and-error. Document which fields are guaranteed present vs. optional.

---

## Tier 4 — Agent-Oriented Domain Features

### Watchlist as named symbol sets

```bash
idx watchlist create banks BBCA,BBRI,BMRI,BNGA
idx stocks quote --watchlist banks
```

Agents build and reuse curated lists across sessions.

### Composite analysis

```bash
idx stocks analyze BBCA -o json
```

Returns quote + fundamentals + ownership + sentiment in one call. Saves agents 4+ tool calls and reduces token overhead.

### Diff / change detection

```bash
idx stocks quote BBCA --diff
```

Compares against cached previous values and surfaces deltas. Agents monitor without maintaining their own state.

### CSV/TSV export

```bash
idx -o csv stocks screen --filter pe_lt:15
```

For agent pipelines that need tabular data (pandas, spreadsheets, database import).

---

## Tier 5 — Integration Hooks

- **Webhook primitives** — `idx watch BBCA --threshold price_gt:5000 --webhook http://...` for event-driven agent architectures
- **Session context** — `idx context set research-session-1` to tag cache entries, enabling isolated research sessions that don't pollute each other

---

## Execution Order

| Priority | Feature | Effort | Impact |
| --- | --- | --- | --- |
| 1 | MCP server mode | Medium | Unlocks all MCP clients |
| 2 | Stdin piping + JSONL | Small | Composability for chaining |
| 3 | Schema export | Small | Agent self-discovery |
| 4 | Watchlist + composite | Medium | Reduces agent round-trips |
| 5 | CSV export | Small | Tabular pipeline support |
| 6 | Webhooks + sessions | Large | Event-driven architecture |

---

## Prerequisites

Close the remaining core gaps first (`FEATURE_SPEC.md` P0/P1):

1. Unify provider and capability flow
2. Fix screener row hygiene
3. Decide fundamentals fallback policy
4. Harden Yahoo reliability edge cases
5. Add `financials` and `earnings` filter flags
