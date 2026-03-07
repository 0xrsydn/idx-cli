# Conventions

## Design Philosophy

### Schema-Driven Development
Define data types FIRST, then build logic around them. Types are the spec.

```rust
// ✅ Good: rich type with documented fields, integer precision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KseiHolding {
    /// Ownership percentage in basis points: 41.10% → 4110.
    pub percentage_bps: i64,
    /// Total shares held (absolute count).
    pub total_shares: i64,
}

// ❌ Bad: stringly-typed, float precision
pub struct Holding {
    pub percentage: f64,       // what unit? what precision?
    pub shares: String,        // why string?
}
```

### Functional Approach
Parse/transform functions are **pure**: take input, return `Result<T, IdxError>`, no side effects.

```rust
// ✅ Good: pure function, testable in isolation
pub fn parse_id_number(s: &str) -> Result<i64, IdxError> { ... }
pub fn normalize_name(raw: &str) -> String { ... }
pub fn parse_quote_from_str(symbol: &str, raw: &str) -> Result<Quote, IdxError> { ... }

// ❌ Bad: function with side effects, hard to test
pub fn fetch_and_save_quote(symbol: &str) -> Result<(), IdxError> { ... }
```

### Types Over Primitives
Use newtypes, enums, and rich structs. Avoid `String` where a domain type exists.

```rust
// ✅ Good
pub struct InvestorTypeCode(pub String);
pub enum Locality { Local, Foreign }
pub enum FlowSignal { Holder, Buyer, Seller, NewPosition, Exited }

// ❌ Bad
pub type InvestorType = String;
pub type Locality = String;
```

## Naming

### Files
- `types.rs` — domain data types for a module
- `mod.rs` — module declarations and re-exports
- `client.rs` — HTTP client code
- `map.rs` / `parse.rs` — response mapping / parsing functions
- `raw_types.rs` — raw API response shapes (before normalization)

### Functions
- `parse_*` — deserialize raw data into domain types
- `normalize_*` — clean/transform data (names, numbers, dates)
- `resolve_*` — lookup/match entities
- `query_*` — read from database
- `fetch_*` — HTTP requests to external APIs
- `render_*` — output formatting (tables, JSON)
- `handle` — CLI command dispatch entry point

### Types
- `*Raw` / `*RawRow` — pre-normalization data (strings from API/PDF)
- `*Holding` — ownership fact row
- `*Metrics` — computed analytics
- `*Row` — display-ready composite type
- `*Args` — clap command arguments

## Patterns

### Provider Trait Pattern
```rust
pub trait QuoteProvider {
    fn quote(&self, symbol: &str) -> Result<Quote, IdxError>;
}

// Factory function, not direct construction
pub fn default_provider(kind: ProviderKind) -> Box<dyn MarketDataProvider> { ... }
```

### Parse Pipeline Pattern
```rust
// Raw API response → domain type, always via parse function
let raw: &str = &response_body;
let quote = yahoo::parse_quote_from_str("BBCA.JK", raw)?;
```

### DB Function Pattern
```rust
// Take &Connection, caller manages lifetime. Use transactions for bulk.
pub fn insert_ksei_holdings(conn: &Connection, holdings: &[KseiHolding]) -> Result<usize, IdxError> { ... }
pub fn query_ticker_holdings(conn: &Connection, code: &str) -> Result<TickerOwnership, IdxError> { ... }
```

### Error Propagation
```rust
// Map external errors to IdxError variants
let conn = Connection::open(path)
    .map_err(|e| IdxError::DatabaseError(e.to_string()))?;
```

## Integer Precision
- **Percentages:** basis points (i64). `41.10%` → `4110`
- **Shares:** absolute count (i64). No floats.
- **Prices:** whole IDR (i64). Rounded from float at parse boundary.
- **Money (USD):** whole dollars (i64) for Bing data.

## Testing
- **Unit tests:** pure functions (parsers, normalizers, signals)
- **Integration tests:** in-memory SQLite (`Connection::open_in_memory()`), mock providers
- **Fixtures:** `tests/fixtures/*.json` — real API responses, sanitized
- **No live API calls in CI** — `IDX_USE_MOCK_PROVIDER=1`
- **Test naming:** `test_<function>_<scenario>` (e.g., `test_parse_id_number_with_dots`)

## Git / VCS
- **jj (Jujutsu)** as local workflow, colocated with git
- Push via `nix develop --command git push` (for prek hooks)
- Branch naming: `feat/<name>`, `fix/<name>`
- Commit messages: conventional commits (`feat:`, `fix:`, `refactor:`, `docs:`)
