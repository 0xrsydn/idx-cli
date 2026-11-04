### New Features

- `idx stocks actions` and `get_corporate_actions` list one stock's corporate actions from your Stockbit session, newest first: cash and stock dividends, bonus shares, rights issues, warrants, splits and reverse splits, shareholders' meetings, public exposes and tender offers. `--kind` picks kinds, and `--kind ipo` adds the stock's IPO. (#78)
- `idx stocks calendar` and `get_corporate_calendar` list the market's actions dated in a range, oldest first: by default today and the next 30 days, at most 366 days. (#78)
- An action's `date` is its cum date where there is an entitlement (dividends, bonuses, rights, splits), else the event's first day. Where Stockbit gives only an ex date, the table marks it `(ex)`. Dividends paid in another currency carry `currency`. (#78)
