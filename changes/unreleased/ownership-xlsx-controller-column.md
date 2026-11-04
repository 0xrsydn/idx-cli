### Bug Fixes

- `ownership import` reads IDX's above-1% register from October 2026 on, which adds a `CONTROLLER_OR_AFFILIATE` column; 0.4.0 rejects these files with `PARSEERROR`. Any other unknown column is still rejected. (#79)
