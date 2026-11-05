### New Features

- `idx ownership movements` and `get_shareholder_movements` list reported changes in insiders' and major holders' stakes from your Stockbit session, newest first. `IDX` rows are insider disclosures with the trade price; `KSEI` rows are holders at 5% and above, with a price Stockbit estimates. Stockbit filters by date, holder, action and source, and `--pages` reads up to 10 pages of 50 rows. If a later page fails, you get the rows read before it, with `incomplete` saying why. (#80)

### Improvements

- The `ownership` commands honour `--offline` and `--no-cache`; only `movements` uses them. (#80)
