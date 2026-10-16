# News Sources Research

Research notes on news beyond MSN: IDX's own news and company disclosures,
and other public sources. Nothing here ships yet; `idx stocks news` and the
`get_company` news section use MSN only (see `docs/ARCHITECTURE.md`), and MSN
is enough for now. Measured 2026-10-16 from the maintainer host unless noted.

## Why

MSN's feed for a stock is often general market news. After relevance ranking,
10 items for ADRO, BREN and CUAN held no company items even 50 deep, and BBRI
one. IDX publishes what MSN lacks: every issuer's disclosures (keterbukaan
informasi), such as monthly shareholder registers, financial statements,
material transactions, and replies to IDX queries, plus the exchange's own
news.

## What blocks direct access

`www.idx.co.id` is behind a Cloudflare JavaScript challenge ("Just a
moment..."). Every page and API below returns `403` with that challenge to
plain curl and to `curl_chrome142` (curl-impersonate, the client ownership
discovery uses), from the maintainer host and from a second datacenter
(greencloud-vps, Singapore, AS8888 xTom). A client that cannot run JavaScript
fails the challenge whatever its IP, so a residential network is unlikely to
help curl; whether a real browser passes as easily from other IPs is
untested.

The Camofox stealth browser on the maintainer host (`127.0.0.1:9377`, a local
service, not shipped) gets through:

1. Open a tab on a page that passes the challenge. The ownership page
   `/id/perusahaan-tercatat/data-kepemilikan-saham/` passed in ~12 s; the
   disclosure page `/id/perusahaan-tercatat/keterbukaan-informasi/` stayed on
   "Performing security verification".
2. `POST /tabs/{id}/navigate` the same tab to an API URL; the clearance cookie
   carries over.
3. `GET /tabs/{id}/snapshot` returns one line, `- text: "<escaped JSON>"`:
   `sed 's/^- text: //' | jq 'fromjson'`.
4. Close the tab. Camofox has no fetch or evaluate endpoint, so JSON comes only
   through the snapshot.

## Endpoints

All under `https://www.idx.co.id`, all JSON once past the challenge.

| Endpoint | Returns |
| --- | --- |
| `/primary/ListedCompany/GetAnnouncement?kodeEmiten=ADRO&emitenType=*&indexFrom=0&pageSize=10&dateFrom=19010101&dateTo=20991231&lang=id&keyword=` | one issuer's disclosures, newest first (ADRO 188, BREN 177) |
| `/primary/NewsAnnouncement/GetAllAnnouncement?keywords=&pageNumber=1&pageSize=100&dateFrom=20261016&dateTo=20261016&lang=id` | every issuer's disclosures in a date range (78 on 2026-10-16) |
| `/primary/NewsAnnouncement/GetNewsSearch?pageNumber=1&pageSize=10&keyword=&lang=id` | exchange news behind `/id/berita/berita` and `/en/news/news` (11,390 items) |

Shapes:

- `GetAnnouncement`: `{ResultCount, Replies: [{pengumuman: {Kode_Emiten,
  JudulPengumuman, PerihalPengumuman, TglPengumuman, NoPengumuman, ...},
  attachments: [{FullSavePath, OriginalFilename, ...}]}]}`. `Kode_Emiten` is
  space-padded (`"ADRO "`).
- `GetAllAnnouncement`: `{ItemCount, PageCount, Items: [{Code, Title, Perihal,
  PublishDate, AnnouncementNo, Attachments: [{FullSavePath, ...}]}]}`. Attachment
  PDFs are under `/StaticData/NewsAndAnnouncement/...`.
- `GetNewsSearch`: `{ItemCount, Items: [{Id, PublishedDate, Title, Summary,
  Tags, IsHeadline, ImageUrl, Links: [{Href: "/api/api/publication/news/<Id>"}]}]}`.

Quirks:

- `GetAllAnnouncement` ignores `kodeEmiten` and returns every issuer; filter on
  `Code`, or use `GetAnnouncement` per issuer.
- `GetNewsSearch` returned the same `en-us` items for `lang=id` and `lang=en`.
- Guessed paths (`/primary/NewsAnnouncement/GetNews`, `/primary/News/GetNews`)
  return `503`, not `404`.
- Exchange news is about IDX itself (meetings, events, regulation), not about
  a given company; it suits a market-news section, not per-ticker news.

## Options

| Option | Verdict |
| --- | --- |
| Users fetch IDX directly (curl-impersonate) | Fails the JavaScript challenge; not viable. |
| Each user runs a stealth browser (idx launches one per fetch) | No: a full browser engine per install and per fetch (hundreds of MB, seconds to start), every user facing the challenge on their own, and IDX seeing thousands of scrapers. |
| Hosted API that fetches IDX on request (e.g. a Cloudflare Worker) | Gets the same challenge; needs a browser anyway. |
| **One publisher fetches on a schedule → static JSON → users** | **Recommended.** Same model as the ownership snapshot. |

One browser, in one place, fetches for everyone; users download small static
files:

```text
       one place, on a schedule                     every user
┌──────────────────────────────────────┐      ┌─────────────────────────┐
│ publisher                            │      │ idx / idx-mcp           │
│  stealth browser, ~2 requests/hour   │ ───> │  plain HTTPS GET of a   │
│  -> normalize -> JSON per ticker     │ JSON │  few-KB JSON file,      │
│  -> upload to a static host          │      │  cached; MSN fallback   │
└──────────────────────────────────────┘      └─────────────────────────┘
```

The browser's cost does not grow with users, and IDX sees one polite client.

## Proposed design

**Publisher** (maintainer host with Camofox, on a schedule):

1. Warm a Camofox tab on a page that passes the challenge.
2. Page `GetAllAnnouncement` for the window since the last run (100 per page;
   a trading day is ~80) and `GetNewsSearch` for new exchange news.
3. Normalize to a small schema: `{ticker, title, subject, published_at,
   number, attachments: [url]}` for disclosures; `{id, title, summary,
   published_at, tags, url}` for news. Trim padding, convert WIB timestamps to
   RFC 3339 with `+07:00`.
4. Keep a rolling window, e.g. 90 days of disclosures (~6,000 items, a few MB
   of JSON; per-ticker shards if size matters) and the latest few hundred news
   items.
5. Publish to a static host (to be chosen, see below). Run hourly on trading
   days; a freshness check alerts when the feed is stale.

**Hosting** (undecided; the project website may serve it later):

| Host | Fit |
| --- | --- |
| GitHub release asset, like `ownership-snapshot-current` | Proven here; suits one bundle (manifest + content-addressed JSON), less so many per-ticker files overwritten hourly. |
| Cloudflare Pages or R2 (the website is on Pages) | A CDN, per-ticker files, cheap frequent overwrites, cache headers for freshness. |
| A small API (a Worker over R2/KV) | Only for queries static files cannot answer, such as all of today's disclosures matching a keyword. |

**Consumer** (idx-core):

- A provider that fetches the JSON over plain HTTPS (no challenge on the
  static host), cached like other providers.
- `stocks news <TICKER>` / `get_company` news: merge the ticker's IDX
  disclosures with MSN articles, each item marked with `source: idx | msn`;
  disclosures count as `relevance: company`.
- Possibly `idx market news` (and an MCP tool) for exchange news.
- If the feed is unreachable, fall back to MSN only, as today.

## Other news sources

| Source | Finding |
| --- | --- |
| MSN (shipped) | Per-stock entity feed, tagged by MSN instrument id. Strong for large caps (BBCA 10/10 company items); for ADRO, BREN and CUAN no company items even 50 deep. Telkom articles are tagged with its NYSE ADR (`TLK`), not `TLKM`. |
| Google News RSS, `https://news.google.com/rss/search?q=<TICKER>+saham+when:7d&hl=id&gl=ID&ceid=ID:id` | Plain HTTP, no key, no browser; each user can fetch directly. Far better per-company coverage: in 7 days, titles naming the ticker: BREN 75/100, GOTO 75/100, CUAN 54/100, BBRI 37/83, TLKM 17/51, ADRO 14/55. Caught the OJK fine on DSSA, BREN and CUAN that MSN missed. Caveats: mixed source quality (real outlets beside low-quality sites, so an allowlist or ranking), links are Google redirect URLs, the feeds are offered for personal use, and some tickers are words ("CUAN" means profit), so match company names too, as `parse_news` does. |
| Yahoo Finance search (`query1/2.finance.yahoo.com/v1/finance/search?q=<T>.JK&newsCount=10`) | `429` from the maintainer host on both hosts; not usable as is. |
| Publisher RSS (Kontan, Bisnis, CNBC Indonesia, Katadata) | Not tested; whole-site feeds, so ticker filtering would be ours. |

If MSN falls short, Google News RSS is the cheapest upgrade: no
infrastructure, and it fits the existing `relevance` labeling. IDX
disclosures are the authoritative source but need the publisher above.

## Camofox notes

- It is a shared local service (`127.0.0.1:9377`), not something to ship; it
  has no per-tab proxy setting, so testing other egress IPs means a separate
  browser on that host.
- JSON comes only through the accessibility snapshot. A raw-content or
  evaluate endpoint would make a publisher sturdier.
- Use your own `userId`, close your own tabs, never `POST /stop`.

## Open questions

- IDX terms of use for automated access and redistributing disclosure
  metadata; IDX also sells official data feeds, the legitimate source if
  scraping is not allowed. Links to IDX-hosted PDFs rather than copies keep the
  footprint small.
- Where the publisher runs (this host with Camofox, or a VPS with its own
  stealth browser), and where its files are hosted.
- Publisher reliability: one browser process; the challenge can change. The
  freshness check and MSN fallback bound the impact.
- Whether `GetAnnouncement` per issuer is needed for backfill beyond the
  rolling window.
