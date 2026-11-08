### New Features

- `ownership discover --require-data-page` fails when the IDX data page can't be read, instead of warning and listing older announcements. The ownership publisher uses it. (#84)

### Bug Fixes

- The ownership publisher no longer reports `RESULT: up-to-date` when only the IDX data page failed and the newest report was missing; it rebuilds and fails loudly instead. (#84)

### Improvements

- A Cloudflare challenge from idx.co.id ends after one round of browser profiles instead of two, since no profile can solve it, and `-v` logs which profile fetched each IDX URL. (#84)

### Output Changes

- **Breaking:** a Cloudflare challenge from idx.co.id now fails with the new error code `CLOUDFLARECHALLENGE` and exit status 6, instead of `HTTP` and exit 4. This affects `ownership discover`, `ownership import --url` and `stocks flow --source idx`; a script that checks for exactly 4 needs updating. (#84)
- The ownership publisher ends a challenged run with `RESULT: blocked stage=<stage>` and exit 6; every other failure stays `RESULT: FAILED`. (#84)
