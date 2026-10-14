# Testing and Smoke Checks

## Automated gate

`nix flake check -L` is the canonical local, pre-push, and CI gate. It runs
offline with pinned tools:

| Check | Covers |
| --- | --- |
| `fmt`, `clippy` | formatting and lints (`--all-targets`) |
| `test` | unit/module tests and `tests/cli.rs` CLI integration tests |
| `default`, `package` | release build and `cargo package` verification |
| `packaged-cli` | the wrapped Nix binary starts, emits JSON, and generates completions |
| `ownership-publisher`, `publisher-test` | packaged publisher entry point and its regression scenarios |
| `install-script-sh`, `install-script-dash`, `shellcheck` | installer regressions and shell lint |

`tests/cli.rs` is the behavioral suite: it runs the real `idx` binary against
fixture-backed mock providers (`IDX_USE_MOCK_PROVIDER=1`) and covers output
contracts, cache/offline transitions, provider routing, error codes, ownership
imports and queries, and snapshot sync.

For a focused loop, run one check or filter Cargo tests:

```bash
nix build .#checks.x86_64-linux.publisher-test -L
nix develop --command cargo test full_screener
```

CI cache keys include platform, dependency/toolchain inputs, and checked source
contents. The native macOS installer job runs separately to exercise BSD tools.

## Live drift check

Fixtures cannot tell when MSN, Yahoo, or IDX change their responses.
`scripts/live-smoke.sh` runs shipped commands against the real upstreams and
validates each JSON output shape. It is opt-in and never part of the gate.

```bash
nix develop --command scripts/live-smoke.sh                   # stocks + ownership
nix develop --command scripts/live-smoke.sh --group stocks --symbol BBRI
nix develop --command scripts/live-smoke.sh --bin ./result/bin/idx
scripts/live-smoke.sh --dry-run
```

- `stocks`: every `stocks` command for one ticker, plus `compare BUMI,ADRO,AIMS`,
  whose MSN key ratios contain non-finite values.
- `ownership`: `discover`, then imports the newest supported report into a temp
  DB and lists `releases`. IDX sits behind Cloudflare and returns `403` to many
  datacenter IPs, including GitHub-hosted runners; run it from a network IDX accepts.

Each run isolates config, cache, and data under `tmp/live-smoke/<timestamp>/`
and keeps per-case `.stdout`, `.stderr`, and `.validation` logs there.

`scripts/audit-msn-fundamentals.sh` is a heavier sweep of valuation output
across the whole IDX MSN symbol map, for provider-health audits.
