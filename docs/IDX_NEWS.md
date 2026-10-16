# IDX News and Disclosures

Research notes and a proposed design for adding IDX's own news and company
disclosures next to MSN news. Nothing here ships yet; `idx stocks news` and the
`get_company` news section use MSN only (see `docs/ARCHITECTURE.md`).
Measured 2026-10-16.

## Why

MSN's feed for a stock is often general market news. After relevance ranking,
10 items for ADRO, BREN and CUAN held no company items even 50 deep, and BBRI
one. IDX publishes what MSN lacks: every issuer's disclosures (keterbukaan
informasi), such as monthly shareholder registers, financial statements,
material transactions, and replies to IDX queries, plus the exchange's own
news.

## What blocks direct access

`www.idx.co.id` is behind a Cloudflare challenge. From the maintainer host
(a datacenter VM), every page and API below returns `403` to plain curl and to
`curl_chrome142` (curl-impersonate), the client ownership discovery uses.
Untested from residential networks.

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
| Users fetch IDX directly (curl-impersonate) | Blocked from datacenter IPs; may work from homes, but unreliable. At most a best-effort fallback. |
| Ship a browser with idx | No: heavy, fragile, and out of place in a CLI and MCP server. |
| Hosted API that fetches IDX on request (e.g. a Cloudflare Worker) | Gets the same challenge; needs a browser anyway. |
| **Maintainer publisher → static JSON → users** | **Recommended.** Same model as the ownership snapshot. |

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
5. Publish to a dedicated GitHub release, `idx-news-current`, as a manifest
   (stable URL, the commit point) plus content-addressed JSON, as
   `docs/OWNERSHIP_PUBLISH.md` does. Run hourly on trading days; a freshness
   check alerts when the manifest is stale.

**Consumer** (idx-core):

- A provider that fetches the manifest and JSON over plain HTTPS (no
  Cloudflare on GitHub release assets), cached like other providers.
- `stocks news <TICKER>` / `get_company` news: merge the ticker's IDX
  disclosures with MSN articles, each item marked with `source: idx | msn`;
  disclosures count as `relevance: company`.
- Possibly `idx market news` (and an MCP tool) for exchange news.
- If the feed is unreachable, fall back to MSN only, as today.

## Open questions

- IDX terms of use for redistributing disclosure metadata and links; links to
  IDX-hosted PDFs rather than copies keep the footprint small.
- Publisher reliability: Camofox is a single local service; the challenge
  behavior can change. The freshness check and MSN fallback bound the impact.
- Whether `GetAnnouncement` per issuer is needed for backfill beyond the
  rolling window.
