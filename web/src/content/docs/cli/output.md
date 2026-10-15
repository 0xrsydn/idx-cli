---
title: Output and exit codes
description: Tables, JSON, errors and exit codes; the contract scripts and agents rely on.
---

## Formats

| `-o` | Prints |
| --- | --- |
| `auto` (default) | a table when stdout is a terminal, compact JSON otherwise |
| `table` | a table |
| `json` | one JSON document |
| `jsonl` | one JSON value per line (one per array item) |

Set the default with `IDX_OUTPUT` or `general.output`. Colour is off whenever
stdout is not a terminal, and with `--no-color`.

## The contract

- stdout carries only data; warnings, progress and errors go to stderr.
- On failure stdout is empty.
- In JSON modes an error is one JSON object on stderr:

  ```json
  {"error":true,"code":"INVALIDINPUT","message":"invalid input: invalid ticker symbol '../x'"}
  ```

## Exit codes

| Exit | Meaning | What to do |
| --- | --- | --- |
| `0` | success | |
| `2` | invalid input or config | fix the command |
| `3` | not found, no data, or no cached data offline | check the ticker or name; go online |
| `4` | upstream unavailable | retry later |
| `5` | unsupported | use another command or provider |
| `1` | anything else | read the message |

## Pipes

```bash
# names and prices
idx stocks quote BBCA BBRI | jq -r '.[] | "\(.symbol) \(.price)"'

# the last five daily closes
idx -o jsonl stocks history BBCA --period 1y | tail -5

# symbols from stdin
idx stocks screen --filter high-dividend | jq -r '.[].symbol' | idx stocks compare -
```

`stocks quote` and `stocks compare` read symbols from stdin when given `-`,
separated by whitespace or commas.
