---
title: About the data
description: Where idx's data comes from, how fresh it is, and the project's terms.
---

## Sources

| Data | Source | Freshness |
| --- | --- | --- |
| Quotes, company data, screening | MSN Money (default) or Yahoo Finance | live, cached for 5 minutes (quotes) or 1 hour (fundamentals) |
| Price history | Yahoo Finance (default) or MSN | live |
| Today's session by the minute (`intraday`), `--provider stockbit` quotes | Stockbit's public symbol page, no login | delayed at least 10 minutes |
| Order book, corporate actions, insider and major holder movements, live quotes, daily history | your own Stockbit session, opt-in (`idx stockbit login`) | live; corporate actions cached for 6 hours |
| Broker summaries and foreign flow (`broker`, `bandar`, `flow`) | Indo Premier (IPOT), or IDX's own site with `flow --source idx` | end of day, per session |
| Ownership | the KSEI register of holders above 1% ("Pemegang Saham di Atas 1%"), published monthly by IDX | the maintained snapshot is checked daily and rebuilt when IDX publishes a new month |

Quotes from these providers can be delayed. The ownership register lists
only holders above 1%; everything below is reported as free float.

Stockbit's terms allow its data for personal, non-commercial use only, which
rules out serving it to other people through `idx-mcp`; see
[Stockbit](/cli/stockbit/#terms).

## The ownership snapshot

`idx ownership sync` downloads a SQLite snapshot built from IDX's published
reports and attached to the
[GitHub release `ownership-snapshot-current`](https://github.com/0xrsydn/idx-cli/releases/tag/ownership-snapshot-current),
with a manifest and SHA-256 checksum. You can build the same database
yourself from IDX's files with `idx ownership import`.

## Terms

idx is open source under the MIT license:
[github.com/0xrsydn/idx-cli](https://github.com/0xrsydn/idx-cli).

It is not affiliated with, endorsed by, or connected to PT Bursa Efek
Indonesia (IDX), PT Kustodian Sentral Efek Indonesia (KSEI), Stockbit, Indo
Premier (IPOT), Microsoft or Yahoo. Data is provided as is, without warranty. Nothing here is investment
advice.
