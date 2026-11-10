---
title: Ownership
description: KSEI shareholder data, synced once and queried offline.
---

The ownership commands work on the monthly KSEI register of holders above 1%
("Pemegang Saham di Atas 1%") that IDX publishes for every listed company.
idx keeps it in a local SQLite database, so queries are instant and work
offline.

## Get the data

```bash
idx ownership sync
```

This downloads a maintained snapshot (a few MB, currently the latest three
months) and installs it. Run it again any time: it does nothing when you are
already current, and it never replaces a database holding more history than
the snapshot. `idx ownership releases` lists the months you have.

## Queries

```bash
idx ownership ticker BBCA               # holders of one stock, with concentration
idx ownership entity "ANTHONI SALIM"    # every holding of one investor
idx ownership search salim              # find investor names
idx ownership changes --from 2026-08-31 --to 2026-09-30
idx ownership concentration --top 20    # most concentrated stocks (by HHI)
idx ownership cross-holders --min-tickers 3
idx ownership graph BBCA --depth 2      # ownership network; --format dot for Graphviz
```

`changes` lists holders that are new, exited, increased or decreased between
two releases. `ticker` also reports concentration: the top holder's share,
the top three, the Herfindahl index (HHI) and the free float (shares held by
holders below 1%).

## Insider and major holder movements

With your own Stockbit login (`idx stockbit login`, see [Stockbit](/cli/stockbit/)),
`movements` lists reported changes in holders' stakes, newest first. It reads
Stockbit live and needs no import:

```bash
idx ownership movements BBCA                        # one stock, 50 rows
idx ownership movements                             # the whole market
idx ownership movements --action sell --source ksei --pages 3
idx ownership movements --from 2026-10-01 --to 2026-10-31
idx ownership movements --holder 1000004481         # one holder's history
```

Two sources share the list. `IDX` rows are insider disclosures: directors,
commissioners and other insiders, at any size, with the trade price. `KSEI`
rows are changes of holders at 5% and above, which report no price; Stockbit
estimates one, shown with `~`. `--holder` takes the number in the HOLDER ID
column. Shares are exact. Stakes are as Stockbit rounds them, so a director's
small holding reads `<0.01%`.

These are filings, not signals. Many insiders buying at one price on one day
is usually a share plan, transfers are not trades, and a row appears days
after the change. Each page is 50 rows and one request to Stockbit (`--pages`
up to 10); a query's pages are read and cached together, for an hour. JSON's
`more` says rows remain past them. If a later page fails, you get the rows
read before it, with `incomplete` saying why, and nothing is cached.

## Import reports yourself

The snapshot covers most needs. To load a month it lacks, or older history,
import IDX's reports directly:

```bash
idx ownership discover                     # find the latest report URLs on IDX
idx ownership discover --require-data-page # fail rather than list only older reports
idx ownership import --url <xlsx-or-pdf>   # needs curl-impersonate
idx ownership import --file report.xlsx    # or a local XLSX, PDF, ZIP or TXT
```

Since June 2026 IDX publishes the register as XLSX; earlier months were PDF
announcements, which need `mutool`. See [helper tools](/start/install/#helper-tools).

When IDX refuses curl, use your browser for both steps. To discover, open
IDX's file list,
<https://www.idx.co.id/primary/list/id/perusahaan-tercatat/data-kepemilikan-saham?lang=id&start=0&length=9999>,
save it, and pass it with `--list-file`; idx reads the file and contacts
nothing. Then download the report and pass its address with `--source-url`, so
`ownership releases` shows where the release came from, as an import by
`--url` would:

```bash
idx ownership discover --list-file list.json
idx ownership import --file peng-2026-10-00029-satu-persen.xlsx \
  --source-url https://www.idx.co.id/Media/wlef2nvt/peng-2026-10-00029-satu-persen.xlsx
```

## Entity names

The register spells some investors differently from month to month.
`idx ownership resolve` lists unresolved names and lets you map an alias to
an entity or merge two entities.
