---
title: Configuration
description: Config file, environment variables and their precedence.
---

idx works without configuration. To change defaults, use the config file,
environment variables or flags. When they disagree, the first one wins:

1. command-line flags
2. environment variables
3. the config file
4. built-in defaults

## Config file

`~/.config/idx/config.toml` (or `$XDG_CONFIG_HOME/idx/config.toml`):

```bash
idx config init                        # write the defaults below
idx config set general.output json
idx config get cache.quote_ttl
idx config path
```

`idx config` prints plain text, even when piped, so `$(idx config path)` works
in scripts. Ask for JSON with `-o json`, `IDX_OUTPUT=json`, or
`output = "json"`.

```toml
[general]
provider = "msn"           # msn | yahoo
history_provider = "auto"  # auto | yahoo | msn
exchange = "JK"
output = "auto"            # auto | table | json | jsonl
color = true

[cache]
quote_ttl = 300            # seconds
fundamental_ttl = 3600

[ownership]
# db_path = "/path/to/ownership.db"
# snapshot_manifest = "https://..."
```

## Environment variables

| Variable | Overrides |
| --- | --- |
| `IDX_PROVIDER` | `general.provider` |
| `IDX_HISTORY_PROVIDER` | `general.history_provider` |
| `IDX_EXCHANGE` | `general.exchange` |
| `IDX_OUTPUT` | `general.output` |
| `IDX_NO_COLOR` | `general.color` (`1` or `true` disables colour) |
| `IDX_CACHE_QUOTE_TTL` | `cache.quote_ttl` |
| `IDX_CACHE_FUNDAMENTAL_TTL` | `cache.fundamental_ttl` |
| `IDX_OWNERSHIP_SNAPSHOT_MANIFEST` | `ownership.snapshot_manifest` |
| `IDX_CURL_IMPERSONATE_BIN` | path to a `curl_chrome*` binary |

## Files idx keeps

| What | Where |
| --- | --- |
| Config | `~/.config/idx/config.toml` |
| Cache | `~/.cache/idx/` (`idx cache info`, `idx cache clear`) |
| Ownership database | `~/.local/share/idx/ownership.db` |

The MCP server reads the same config, cache and database.

## Offline and caching

Quotes are cached for `quote_ttl` seconds and fundamentals for
`fundamental_ttl`. `--offline` makes no network requests and serves cached
data, even when stale; `--no-cache` always fetches.

:::note
A config created by `idx config init` before v0.3 pins `output = "table"`.
Run `idx config set general.output auto` to get JSON when piped.
:::
