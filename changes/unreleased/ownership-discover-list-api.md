### New Features

- `ownership discover --list-file <json>` reads IDX's report list saved from a browser, contacting nothing, so when IDX refuses curl you can still find a report and import it with `import --file --source-url`. (#86)

### Improvements

- `ownership discover` reads IDX's report list from the JSON its data page loads, one small request covering every year, instead of picking it out of the page's HTML, which breaks when IDX changes the page. (#86)

### Output Changes

- In `ownership discover` JSON, a data page report's `query_url` is now the list request (`https://www.idx.co.id/primary/list/id/perusahaan-tercatat/data-kepemilikan-saham?lang=id&start=0&length=9999`) instead of the page address; `listing_page_url` is still the page. A snapshot published from it records the same in `source.query_url`. (#86)

### Notes

- The test override `IDX_OWNERSHIP_DATA_PAGE_URL` is replaced by `IDX_OWNERSHIP_DATA_LIST_URL`, the address of the list. (#86)
