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

## Import reports yourself

The snapshot covers most needs. To load a month it lacks, or older history,
import IDX's reports directly:

```bash
idx ownership discover                     # find the latest report URLs on IDX
idx ownership import --url <xlsx-or-pdf>   # needs curl-impersonate
idx ownership import --file report.xlsx    # or a local XLSX, PDF, ZIP or TXT
```

Since June 2026 IDX publishes the register as XLSX; earlier months were PDF
announcements, which need `mutool`. See [helper tools](/start/install/#helper-tools).

## Entity names

The register spells some investors differently from month to month.
`idx ownership resolve` lists unresolved names and lets you map an alias to
an entity or merge two entities.
