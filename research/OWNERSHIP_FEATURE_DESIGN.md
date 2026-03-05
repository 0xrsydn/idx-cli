# IDX Ownership Intelligence Feature Design (KSEI >1% Dataset)

## 1) What I analyzed

### Source file
- URL: `https://www.idx.co.id/StaticData/NewsAndAnnouncement/ANNOUNCEMENTSTOCK/From_EREP/202603/95f8c4c8bc_848269e900.pdf`
- Saved to: `/var/lib/openclaw/projects/idx-cli/research/ownership_202603.pdf`
- Download note: direct `curl` got Cloudflare block HTML; download succeeded using browser-like headers via Python `urllib`.

### Parsing approach
- `pdftotext` not available in runtime.
- Used Node parser stack:
  - `pdf-parse` for full-text sanity check
  - `pdf2json` for coordinate-based extraction (critical for column integrity)
- Extracted row-level dataset to:
  - `/var/lib/openclaw/projects/idx-cli/research/ownership_202603_rows.ndjson`
  - `/var/lib/openclaw/projects/idx-cli/research/ownership_202603_analysis.json`

---

## 2) PDF structure analysis

### High-level structure
- Total pages: **73**
- First pages: cover letter/explanatory text from KSEI.
- Tabular section (core data): repeated row records with these headers:

```text
DATE
SHARE_CODE
ISSUER_NAME
INVESTOR_NAME
INVESTOR_TYPE
LOCAL_FOREIGN
NATIONALITY
DOMICILE
HOLDINGS_SCRIPLESS
HOLDINGS_SCRIP
TOTAL_HOLDING_SHARES
PERCENTAGE
```

### Observed dataset size (from parsed rows)
- Total ownership rows: **7,257**
- Unique tickers (`share_code`): **955**
- Unique issuer names: **956** (1 extra due naming variant/noise)
- Unique investor names (raw uppercased): **5,195**
- As-of date: **27-Feb-2026** for all rows in this release

### Example extracted rows

```json
{
  "date": "27-Feb-2026",
  "share_code": "AADI",
  "issuer_name": "ADARO ANDALAN INDONESIA Tbk",
  "investor_name": "ADARO STRATEGIC INVESTMENTS",
  "investor_type": "CP",
  "local_foreign": "L",
  "nationality": "",
  "domicile": "INDONESIA",
  "holdings_scripless": "3.200.142.830",
  "holdings_scrip": "0",
  "total_holding_shares": "3.200.142.830",
  "percentage": "41,10"
}
```

```json
{
  "date": "27-Feb-2026",
  "share_code": "AALI",
  "issuer_name": "ASTRA AGRO LESTARI Tbk",
  "investor_name": "PT ASTRA INTERNATIONAL TBK",
  "investor_type": "CP",
  "local_foreign": "L",
  "nationality": "",
  "domicile": "INDONESIA",
  "holdings_scripless": "0",
  "holdings_scrip": "1.533.682.440",
  "total_holding_shares": "1.533.682.440",
  "percentage": "79,68"
}
```

### Data quality notes
- Missingness:
  - `investor_type` missing: 244 rows
  - `local_foreign` missing: 244 rows
  - `nationality` missing: 4,240 rows
  - `domicile` missing: 1,053 rows
- Investor types observed most: `CP`, `ID`, `IB`, `MF`, `SC`, `OT`, `IS`
- Raw entity names have normalization issues (`PT`, punctuation, case, suffix variations).

---

## 3) Analytical potential from this release

### Cross-holder signal (same investor across many tickers)
Top examples (raw name grouping):
- UOB KAY HIAN PRIVATE LIMITED: **66** tickers
- BANK OF SINGAPORE LIMITED: **38**
- PT. ASABRI (Persero): **33**
- DJS Ketenagakerjaan (JHT): **31**
- UBS AG Singapore Branch: **27**

This already forms a strong bipartite graph: `entity -> owns -> ticker`.

### Concentration metrics per ticker
Examples from parsed results:
- Very concentrated:
  - `IBST`: 1 holder >1%, total captured 99.95%
  - `SUPR`: largest holder 97.33%
- More dispersed among >1% holders:
  - `BBRI`: total >1% captured 5.86%
  - `ADHI`: 7.37%
  - `BBNI`: 7.95%

### Breadth metric
- Tickers with most >1% holders in this snapshot:
  - `CARS` (28), `INPC` (28), `BOGA` (27), etc.

---

## 4) Proposed data model (SQLite-first)

Use SQLite for local analytics in `idx-cli` (fast, portable, no server dependency).

### Core tables

```sql
-- One PDF release/event
CREATE TABLE ownership_release (
  id INTEGER PRIMARY KEY,
  source_url TEXT NOT NULL,
  source_file_sha256 TEXT NOT NULL UNIQUE,
  as_of_date TEXT NOT NULL,            -- YYYY-MM-DD
  published_at TEXT,
  fetched_at TEXT NOT NULL,
  parser_version TEXT NOT NULL,
  row_count INTEGER NOT NULL,
  metadata_json TEXT
);

CREATE TABLE issuer (
  id INTEGER PRIMARY KEY,
  ticker TEXT NOT NULL UNIQUE,
  issuer_name_raw TEXT NOT NULL,
  issuer_name_norm TEXT NOT NULL
);

CREATE TABLE entity (
  id INTEGER PRIMARY KEY,
  canonical_name TEXT NOT NULL,
  canonical_name_norm TEXT NOT NULL UNIQUE,
  entity_kind TEXT,                    -- company/person/fund/gov/unknown
  country_hint TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

-- Raw name variants resolved to entity
CREATE TABLE entity_alias (
  id INTEGER PRIMARY KEY,
  entity_id INTEGER NOT NULL REFERENCES entity(id),
  alias_raw TEXT NOT NULL,
  alias_norm TEXT NOT NULL,
  confidence REAL NOT NULL,            -- 0..1
  method TEXT NOT NULL,                -- exact/manual/fuzzy/rule
  UNIQUE(entity_id, alias_norm)
);

CREATE TABLE ownership_fact (
  id INTEGER PRIMARY KEY,
  release_id INTEGER NOT NULL REFERENCES ownership_release(id),
  issuer_id INTEGER NOT NULL REFERENCES issuer(id),
  entity_id INTEGER NOT NULL REFERENCES entity(id),

  investor_name_raw TEXT NOT NULL,
  investor_type TEXT,
  local_foreign TEXT,
  nationality TEXT,
  domicile TEXT,

  holdings_scripless INTEGER NOT NULL,
  holdings_scrip INTEGER NOT NULL,
  total_holding_shares INTEGER NOT NULL,
  percentage_bps INTEGER NOT NULL,     -- e.g. 41.10% => 4110

  -- one row per entity/ticker/release/rawname (can enforce stronger uniqueness later)
  UNIQUE(release_id, issuer_id, investor_name_raw)
);
```

### Useful indexes

```sql
CREATE INDEX idx_fact_release_issuer ON ownership_fact(release_id, issuer_id);
CREATE INDEX idx_fact_release_entity ON ownership_fact(release_id, entity_id);
CREATE INDEX idx_fact_pct ON ownership_fact(release_id, percentage_bps DESC);
CREATE INDEX idx_alias_norm ON entity_alias(alias_norm);
```

---

## 5) End-to-end pipeline design (PDF → queryable intelligence)

1. **Fetch**
   - Download announcement PDF using robust HTTP headers.
   - Store raw file in data dir + SHA256.

2. **Parse (bronze)**
   - Coordinate extraction from PDF (x/y text cells).
   - Emit raw row JSON with strict schema + parser warnings.

3. **Normalize (silver)**
   - Parse numerics:
     - `1.533.682.440` → integer shares
     - `79,68` → 7968 bps
   - Standardize date: `27-Feb-2026` → `2026-02-27`
   - Normalize text fields (trim, whitespace, uppercase key columns).

4. **Resolve entities (gold)**
   - Deterministic normalization rules (`PT.`, commas, suffixes, punctuation).
   - Alias mapping table + manual overrides.
   - Fuzzy match only with high threshold + review queue.

5. **Load SQLite**
   - UPSERT `issuer`, `entity`, `entity_alias`, `ownership_fact`.
   - Keep release snapshots immutable for time-series diffs.

6. **Derive marts/materialized views**
   - `v_ticker_concentration` (HHI, top1, top3, sum>1)
   - `v_entity_cross_holdings` (#tickers, total bps)
   - `v_pair_coownership` (entity pairs co-appearing across tickers)

---

## 6) Entity graph design

### Graph model
- **Node types**:
  - `Entity` (investor)
  - `Ticker` (issuer)
- **Edge**: `OWNS` with attributes
  - `release_id`, `percentage_bps`, `shares_total`, `investor_type`, `local_foreign`

### Derived graph analytics
1. **Cross-holders**: entities with high ticker degree.
2. **Co-ownership network**:
   - Build `Entity --co_owns--> Entity` weighted by number of shared tickers and summed min(%).
3. **Cluster detection**:
   - Louvain / connected components on co-ownership graph.
4. **Concentration**:
   - per ticker `top1`, `top3`, `sum_pct_gt1`, `HHI`.
5. **Temporal graph** (when monthly releases accumulate):
   - edge delta (`+/- bps`), entry/exit events, emerging cluster shifts.

---

## 7) CLI command proposals

Integrate as a new top-level group in `SPEC.md` style:

```text
idx ownership
├── ticker <SYMBOL>                # holders >1% for ticker
├── entity <NAME_OR_ID>            # what this entity owns
├── cross-holders                  # entities with widest cross-ownership
│   [--top 20] [--min-tickers 5]
├── concentration                  # ranking by concentration metrics
│   [--by top1|top3|sum|hhi] [--top 20] [--least]
├── clusters                       # co-ownership clusters
│   [--min-shared 2]
├── changes                        # compare two releases
│   --from <YYYY-MM-DD> --to <YYYY-MM-DD>
├── releases                       # available snapshots
├── import                         # parse & load latest PDF(s)
│   [--url <PDF_URL>] [--file <PATH>] [--as-of <DATE>] [--force]
└── resolve                        # alias/entity management
    ├── list-unresolved
    ├── map <ALIAS> <ENTITY>
    └── merge <ENTITY_A> <ENTITY_B>
```

### Example UX

```bash
$ idx ownership ticker BBCA
AS OF: 2026-02-27 | TICKER: BBCA
RANK  INVESTOR                               TYPE  L/F  SHARES          %
1     PT ...                                  CP    L    12,345,678,900  54.32
2     ...
```

```bash
$ idx -o json ownership entity "UOB KAY HIAN PRIVATE LIMITED"
{
  "entity": "UOB KAY HIAN PRIVATE LIMITED",
  "as_of": "2026-02-27",
  "ticker_count": 66,
  "holdings": [ ... ]
}
```

---

## 8) Architecture recommendations

### Local-first (recommended MVP)
- DB path: `~/.local/share/idx/ownership.db` (or XDG equivalent)
- Raw files cache: `~/.cache/idx/ownership/raw/`
- Parsed snapshots: `~/.cache/idx/ownership/parsed/`

Pros: offline-capable, instant query, aligns with existing CLI/caching philosophy in `SPEC.md`.

### Update model
- `idx ownership import` checks known URL(s) or accepts explicit URL/file.
- De-duplicate by SHA256 + as_of_date.
- Keep all snapshots to unlock `changes` and trend analytics.

### Optional future API mode
- If multi-user/team usage needed, same schema can back a lightweight API service.
- Keep CLI query layer repository-backed so data source can be local SQLite or remote API.

---

## 9) Entity resolution challenges (critical)

1. **Name variants**: `PT X`, `PT. X`, `X PT`, punctuation/case.
2. **Corporate suffix permutations**: `TBK`, `Tbk`, `(PERSERO)`, etc.
3. **Custodian omnibus names** may mask underlying beneficial owners.
4. **Person name ambiguity** (same personal names).
5. **Cross-language spelling** and abbreviations.
6. **Corporate group mapping** (subsidiary vs parent): separate from exact legal entity identity.

### Practical strategy
- Phase 1: conservative exact+rule normalization (high precision).
- Phase 2: human-reviewed alias map.
- Phase 3: optional fuzzy suggestions with confidence score, never auto-merge below threshold.

---

## 10) Integration with current `idx-cli` SPEC

Current `SPEC.md` is quote/fundamental/technical-centric. Ownership feature fits as a differentiated analytics vertical:

- Add new top-level command group: `ownership`
- Reuse global output modes: `table|json|csv|tsv`
- Reuse config precedence (flags > env > config)
- Extend config:

```toml
[ownership]
db_path = "~/.local/share/idx/ownership.db"
raw_cache_dir = "~/.cache/idx/ownership/raw"
parsed_cache_dir = "~/.cache/idx/ownership/parsed"
entity_resolution_mode = "conservative"
auto_import = false
```

- Add milestone slice (proposed):
  - **v0.2.5**: `ownership import`, `ownership ticker`, `ownership entity`
  - **v0.3**: concentration/cross-holder rankings, releases/changes
  - **v0.4+**: clustering/graph exports and manual resolution workflow

---

## 11) Open questions before implementation

1. Official stable source URL pattern for future monthly releases?
2. Will data always be PDF only, or also XLS/CSV endpoint?
3. Canonical meaning of investor type codes (`CP`, `ID`, `IB`, etc.) — need official codebook.
4. Should ADR/dual-listing/suspended symbols be filtered in CLI output?
5. Entity resolution governance: where to store curated alias mappings in-repo vs user-local?

---

## 12) Key files generated in this research

- `/var/lib/openclaw/projects/idx-cli/research/ownership_202603.pdf`
- `/var/lib/openclaw/projects/idx-cli/research/ownership_202603_extracted.txt`
- `/var/lib/openclaw/projects/idx-cli/research/ownership_202603_rows.ndjson`
- `/var/lib/openclaw/projects/idx-cli/research/ownership_202603_analysis.json`
- `/var/lib/openclaw/projects/idx-cli/research/OWNERSHIP_FEATURE_DESIGN.md`

These provide a concrete parsed sample and can be used directly to bootstrap implementation + tests.
