### Output Changes

- MCP answers and errors carry `warnings` when idx worked around something the CLI warns about on stderr: history that fell back to Yahoo, a stale cache entry and the failure behind it, or a shareholder movements query cut short. (#81)

### Improvements

- The stale-cache warning names the failure that caused it, for example `network failed (provider unavailable), serving stale cache for BBCA.JK`. (#81)
