# idx-cli — System Design & Project Blueprint

> CLI tool for Indonesian stock market (IDX) analysis. Built for humans and AI agents.

## References

These CLIs informed the design patterns used in this spec:

- **[Google Workspace CLI (`gws`)](https://github.com/googleworkspace/cli)** — Dynamic command surface from API discovery, strict auth precedence (flag > env > config), encrypted credential store, MCP mode, schema introspection commands. Great model for agent discoverability.
- **[Polymarket CLI](https://github.com/Polymarket/polymarket-cli)** — Clean domain-based command hierarchy (`markets`, `events`, `wallet`), `--output table|json` global flag, config file + env overrides + flags precedence, interactive shell mode. Direct template for our command tree and output modes.
- **[Obsidian CLI](https://help.obsidian.md/cli)** — `group:subcommand` naming, multi-format output (`json|csv|tsv|md`), TUI/shell mode + one-shot mode, clear docs on execution context and caching behavior. Good reference for help ergonomics.

---

## Goals

1. **Fast, single-binary CLI** — Rust + clap, zero runtime deps
2. **Human-first, agent-friendly** — readable tables by default, `--json` for machines
3. **Self-documenting** — agents discover capabilities via `--help` alone
4. **Composable** — pipe, script, batch — standard Unix CLI philosophy
5. **Offline-safe** — graceful degradation, local caching, no crashes on network failure

---

## Data Sources

### Primary: Yahoo Finance (unofficial HTTP API)
- Real-time quotes, fundamentals, historical OHLC
- No API key required
- Endpoint: `https://query1.finance.yahoo.com/v8/finance/chart/{SYMBOL}.JK`
- Fundamentals: `https://query1.finance.yahoo.com/v10/finance/quoteSummary/{SYMBOL}.JK`
- Rate limits: ~2000 req/hr (undocumented, we should respect ~1 req/s burst)

### Future (pluggable):
- IDX official API (if/when available)
- Alpha Vantage, Twelve Data, etc. via `idx config set provider ...`

---

## Command Tree

```
idx
├── stocks
│   ├── quote <SYMBOL...>           # Price, change, volume, 52w range
│   ├── technical <SYMBOL>          # RSI, MACD, signals
│   ├── fundamental <SYMBOL>        # Composite: growth + valuation + risk
│   ├── growth <SYMBOL>             # Revenue/earnings growth
│   ├── valuation <SYMBOL>          # PE, PB, ROE, margins, EV/EBITDA
│   ├── risk <SYMBOL>               # D/E, current ratio, ROA
│   ├── compare <SYM1,SYM2,...>     # Side-by-side multi-symbol comparison
│   │   [--metrics price,valuation,technical,growth,risk]
│   └── history <SYMBOL>            # Historical OHLC data
│       [--period 1d|5d|1mo|3mo|6mo|1y|2y|5y]
│       [--interval 1d|1wk|1mo]
│
├── market
│   ├── summary                     # IHSG index, market breadth
│   ├── movers                      # Top gainers/losers/volume
│   │   [--by gainers|losers|volume] [--top 10]
│   └── sectors                     # Sector performance overview
│
├── screen
│   ├── query "<EXPR>"              # Filter stocks by expression
│   │   e.g. "pe < 15 and roe > 20 and market_cap > 1T"
│   ├── presets                     # List built-in screen presets
│   └── run <PRESET>                # Run a named preset
│
├── watchlist
│   ├── list                        # Show all watchlists
│   ├── create <NAME>               # Create new watchlist
│   ├── delete <NAME>               # Delete watchlist
│   ├── add <NAME> <SYMBOL...>      # Add symbols
│   ├── remove <NAME> <SYMBOL...>   # Remove symbols
│   ├── show <NAME>                 # Show watchlist with live quotes
│   └── watch <NAME>                # Live terminal refresh
│       [--interval 30s]
│
├── alerts                          # (v0.2+)
│   ├── list
│   ├── add --symbol <SYM> --when "<EXPR>"
│   ├── remove <ID>
│   └── daemon                      # Background alert checker
│
├── cache
│   ├── info                        # Cache stats
│   └── clear                       # Purge cache
│
├── config
│   ├── init                        # Create default config
│   ├── get <KEY>
│   ├── set <KEY> <VALUE>
│   └── path                        # Print config file path
│
├── completions <SHELL>             # Generate shell completions
│   [bash|zsh|fish|powershell]
│
└── version
```

---

## Symbol Resolution

- Input: `BBCA` → resolved to `BBCA.JK` (IDX suffix)
- Input: `BBCA.JK` → used as-is
- Default exchange suffix configurable: `idx config set exchange JK`
- Multiple symbols: comma-separated `BBCA,BBRI,BMRI` or space-separated where noted

---

## Output Strategy

### Global flags
```
-o, --output <FORMAT>    Output format [default: table]
                         [table, json, csv, tsv]
    --no-color           Disable colored output
-q, --quiet              Suppress non-essential output
-v, --verbose            Increase verbosity
```

### Table mode (default, for humans)
```
$ idx stocks quote BBCA
SYMBOL    PRICE    CHG     CHG%    VOLUME   MKT CAP     52W RANGE    SIGNAL
BBCA.JK   9,875   +117   +1.20%   12.3M    1,215.2T    ████████░░   upper
```

### JSON mode (for agents/scripts)
```json
$ idx -o json stocks quote BBCA
{
  "symbol": "BBCA.JK",
  "price": 9875,
  "change": 117,
  "change_pct": 1.20,
  "volume": 12300000,
  "market_cap": 1215200000000000,
  "week52_high": 10250,
  "week52_low": 7800,
  "week52_position": 0.732,
  "range_signal": "upper"
}
```

### CSV/TSV mode (for batch/spreadsheet)
```
$ idx -o csv stocks quote BBCA,BBRI
symbol,price,change,change_pct,volume,market_cap
BBCA.JK,9875,117,1.20,12300000,1215200000000000
BBRI.JK,4560,45,1.00,25600000,567800000000000
```

### Error handling
- Exit code 0 on success, non-zero on failure
- Table mode: human error on stderr
- JSON mode: `{"error": true, "code": "SYMBOL_NOT_FOUND", "message": "..."}`

---

## Technical Analysis Implementation

### Indicators (v0.1)
- **RSI(14)** — Relative Strength Index, 14-period
- **MACD(12,26,9)** — Moving Average Convergence Divergence
- **SMA(20,50,200)** — Simple Moving Averages
- **Volume analysis** — vs 20-day average

### Signal interpretation
Each indicator produces a signal: `bullish | bearish | neutral`

Overall technical signal derived from weighted consensus:
- RSI: overbought (>70) / oversold (<30) / neutral
- MACD: histogram direction + signal line cross
- Price vs SMA: above/below 50/200 day

### Fundamental metrics
- **Growth**: revenue growth, earnings growth YoY
- **Valuation**: trailing PE, forward PE, PB, EV/EBITDA, ROE, profit margin
- **Risk**: D/E ratio, current ratio, ROA

Each category produces an interpreted signal with the raw numbers.

---

## Screening Engine (v0.1)

Simple expression parser for filtering stocks:

```
idx screen query "pe < 15 and roe > 20"
idx screen query "market_cap > 100T and dividend_yield > 3"
idx screen query "rsi < 30"  # oversold screen
```

### Available fields
`price`, `change_pct`, `volume`, `market_cap`, `pe`, `pb`, `roe`, `roa`,
`de_ratio`, `current_ratio`, `profit_margin`, `revenue_growth`,
`earnings_growth`, `dividend_yield`, `rsi`, `week52_position`

### Operators
`>`, `<`, `>=`, `<=`, `==`, `!=`, `and`, `or`

### Built-in presets
- `value` — PE < 15, PB < 1.5, ROE > 15
- `growth` — revenue growth > 20%, earnings growth > 20%
- `oversold` — RSI < 30, week52_position < 0.3
- `dividend` — dividend yield > 4%, payout sustainable
- `blue-chip` — market cap > 100T, ROE > 15

---

## Caching

- **Location**: `~/.cache/idx/`
- **Strategy**: file-based, keyed by (symbol, data_type, params)
- **Default TTL**: 5 minutes for quotes, 1 hour for fundamentals
- **Format**: binary (bincode/msgpack) for speed
- Configurable: `idx config set cache.quote_ttl 300`

---

## Configuration

### File: `~/.config/idx/config.toml`
```toml
[general]
exchange = "JK"
output = "table"
color = true

[cache]
quote_ttl = 300        # seconds
fundamental_ttl = 3600

[provider]
default = "yahoo"
# alpha_vantage_key = "..."  # future
```

### Precedence
`flags > env vars > config file > defaults`

### Environment variables
```
IDX_OUTPUT=json
IDX_EXCHANGE=JK
IDX_CACHE_QUOTE_TTL=300
IDX_NO_COLOR=1
```

---

## Project Structure

```
idx-cli/
├── Cargo.toml
├── SPEC.md               # This file
├── README.md
├── LICENSE                # MIT
├── src/
│   ├── main.rs            # Entry point, clap app setup
│   ├── cli/
│   │   ├── mod.rs
│   │   ├── stocks.rs      # stocks subcommands
│   │   ├── market.rs      # market subcommands
│   │   ├── screen.rs      # screen subcommands
│   │   ├── watchlist.rs   # watchlist subcommands
│   │   ├── alerts.rs      # alerts subcommands (v0.2)
│   │   ├── cache.rs       # cache subcommands
│   │   └── config.rs      # config subcommands
│   ├── api/
│   │   ├── mod.rs
│   │   ├── yahoo.rs       # Yahoo Finance HTTP client
│   │   └── types.rs       # API response types
│   ├── analysis/
│   │   ├── mod.rs
│   │   ├── technical.rs   # RSI, MACD, SMA
│   │   ├── fundamental.rs # Growth, valuation, risk
│   │   └── signals.rs     # Signal interpretation
│   ├── screen/
│   │   ├── mod.rs
│   │   ├── parser.rs      # Expression parser
│   │   └── presets.rs     # Built-in presets
│   ├── output/
│   │   ├── mod.rs
│   │   ├── table.rs       # Rich table formatting
│   │   ├── json.rs        # JSON output
│   │   └── csv.rs         # CSV/TSV output
│   ├── cache.rs           # File-based caching
│   ├── config.rs          # Config loading/merging
│   └── error.rs           # Error types
└── tests/
    ├── integration/
    └── fixtures/
```

---

## Crate Dependencies (expected)

```toml
[dependencies]
clap = { version = "4", features = ["derive"] }
reqwest = { version = "0.12", features = ["json", "rustls-tls"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
comfy-table = "7"           # table rendering
colored = "2"               # terminal colors
toml = "0.8"                # config parsing
directories = "5"           # XDG paths
chrono = "0.4"
thiserror = "2"
```

---

## Milestones

### v0.1 — Core (MVP)
- [ ] Project scaffold + CI
- [ ] Yahoo Finance API client (quotes + fundamentals + history)
- [ ] `stocks quote`, `stocks technical`, `stocks fundamental`
- [ ] `stocks growth`, `stocks valuation`, `stocks risk`
- [ ] `stocks compare`, `stocks history`
- [ ] Table + JSON output
- [ ] Symbol resolution (auto `.JK`)
- [ ] File-based caching
- [ ] Config system
- [ ] Shell completions
- [ ] `--help` on every command with examples

### v0.2 — Market & Screening
- [ ] `market summary`, `market movers`, `market sectors`
- [ ] Screening engine (expression parser + presets)
- [ ] CSV/TSV output
- [ ] Watchlists (local file-based)

### v0.3 — Live & Alerts
- [ ] `watchlist watch` (live terminal refresh)
- [ ] Alert engine + daemon mode
- [ ] Notification hooks (stdout, webhook, etc.)

### v0.4 — Distribution
- [ ] Nix package
- [ ] Homebrew formula
- [ ] GitHub releases (cross-compiled binaries)
- [ ] `cargo install idx-cli`

---

## Agent Skills

Inspired by [Google Workspace CLI's skills system](https://github.com/googleworkspace/cli/tree/main/skills), `idx-cli` ships a `skills/` directory with SKILL.md files that teach AI agents how to use the CLI effectively. No MCP, no JSON schema bloat — just markdown instructions that any agent framework can pick up.

### Structure

```
skills/
├── idx-shared/SKILL.md          # Install block, common patterns, output modes
├── idx-quote/SKILL.md           # Price lookup, multi-symbol quotes
├── idx-technical/SKILL.md       # Technical analysis workflow
├── idx-fundamental/SKILL.md     # Fundamental analysis (growth, valuation, risk)
├── idx-compare/SKILL.md         # Multi-stock comparison
├── idx-screen/SKILL.md          # Stock screening with expressions & presets
├── idx-ownership/SKILL.md       # Ownership intelligence queries
├── idx-watchlist/SKILL.md       # Watchlist management
├── idx-workflow-dd/SKILL.md     # Due diligence workflow (chains multiple commands)
└── idx-workflow-sector/SKILL.md # Sector analysis workflow
```

### Skill anatomy

Each SKILL.md follows a consistent format:

```markdown
# idx-quote — Stock Price Lookup

## Install
<!-- Auto-install block for agent frameworks -->
```bash
cargo install idx-cli  # or: nix run github:0xrsydn/idx-cli
```

## Commands
<!-- Exact commands with examples -->
idx stocks quote BBCA
idx stocks quote BBCA,BBRI,BMRI -o json

## Output format
<!-- What the agent should expect back -->

## Patterns
<!-- Common usage patterns, gotchas, tips -->

## See also
<!-- Related skills -->
```

### Integration with agent frameworks

```bash
# OpenClaw — symlink all skills
ln -s /path/to/idx-cli/skills/idx-* ~/.openclaw/skills/

# Or install specific skills
cp -r skills/idx-quote skills/idx-ownership ~/.openclaw/skills/

# Claude Code — skills are auto-discovered from repo
# Gemini CLI — same pattern as gws
```

### Design principles

1. **Self-contained** — each skill has everything an agent needs, no cross-references required
2. **Example-driven** — real commands with real output, not abstract descriptions
3. **Composable** — workflow skills reference atomic skills, agents can chain them
4. **Framework-agnostic** — plain markdown works with OpenClaw, Claude, Gemini, Cursor, etc.

---

## Open Questions

1. **Stock universe for screening** — Yahoo doesn't have a "list all IDX stocks" endpoint. We need a static list of IDX symbols (~800) bundled or fetched from IDX website. How to maintain?
2. **Rate limiting strategy** — batch requests vs sequential with delay? Should we parallelize multi-symbol queries?
3. **Offline mode** — serve from cache when network unavailable, or fail explicitly?
4. **Interactive shell** — worth building `idx shell` REPL in v0.1, or defer?
5. **Plugin system** — allow custom analyzers/data sources via dynamic loading, or keep it simple?
