### Bug Fixes

- `ownership discover`, `ownership import --url` and `stocks flow --source idx` no longer send a fixed browser User-Agent over every curl-impersonate profile, which did not match the profile's fingerprint and drew Cloudflare challenges from IDX; each profile now sends its own. (#PR)
