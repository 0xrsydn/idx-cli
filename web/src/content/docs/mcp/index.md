---
title: MCP server
description: idx-mcp serves idx's stock and ownership data to AI agents over MCP.
---

`idx-mcp` is a [Model Context Protocol](https://modelcontextprotocol.io)
server over stdio. It gives an agent the same data as the `idx` CLI, as
[11 tools](/mcp/tools/): quotes, price history, screening, company data,
analysis, comparison, and the KSEI ownership queries.

It shares idx's [config, cache and ownership database](/start/config/#files-idx-keeps),
so `idx ownership sync` in a terminal also feeds the server.

[Set it up in your agent →](/mcp/setup/)

## Results

Tools that fetch live data return:

```json
{
  "data": { "...": "the same shape as idx -o json" },
  "source": "msn",
  "fetched_at": "2026-10-15T03:20:11Z",
  "stale": false
}
```

`source` is the provider (`msn` or `yahoo`). Results are cached on disk;
`stale` is true when an expired cache entry was served because a fresh fetch
failed. Ownership tools return their data directly, from the local
database.

Errors come back as a tool result with `isError` set and the CLI's error
body, so the agent can decide whether to retry:

```json
{"error": true, "code": "PROVIDERUNAVAILABLE", "message": "..."}
```

## Limits

| Input | Limit |
| --- | --- |
| tickers per call (`get_quotes`, `compare_stocks`) | 20, duplicates removed |
| `screen_stocks` `limit` | 200 |
| `search_investors` `limit` | 100 |
| `get_ownership_changes` `limit` | 2000 (default 200) |
| concurrent tool calls | 4 |

## Ownership data

Ownership tools need a populated database. Either run `idx ownership sync`
once, or start the server with `--allow-sync`, which adds an
`ownership_sync` tool so the agent can download the snapshot itself (several
MB).
