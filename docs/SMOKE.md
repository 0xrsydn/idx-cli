# Testing and Smoke Checks

## Automated gate

`nix flake check -L` is the canonical local, pre-push, and CI gate. It runs
offline with pinned tools:

| Check | Covers |
| --- | --- |
| `fmt`, `clippy` | formatting and lints (`--all-targets`) |
| `test` | unit/module tests, `tests/cli.rs` CLI integration tests, `tests/golden.rs` output parity, and `crates/idx-mcp/tests/mcp.rs` MCP stdio tests |
| `default`, `package` | release build and `cargo package` verification |
| `packaged-cli` | the wrapped Nix release binary emits JSON, generates completions, and returns a JSON `CACHEMISS` error with exit 3 offline |
| `ownership-publisher`, `publisher-test` | packaged publisher entry point and its regression scenarios |
| `install-script-sh`, `install-script-dash`, `shellcheck` | installer regressions and shell lint |

`tests/cli.rs` is the behavioral suite: it runs the real `idx` binary against
fixture-backed mock providers (`IDX_USE_MOCK_PROVIDER=1`) and covers output
contracts, cache/offline transitions, provider routing, error codes, ownership
imports and queries, and snapshot sync.

`tests/golden.rs` pins the exact output (exit code, stdout, stderr) of every
command listed in `tests/golden/cases.toml`, in table and JSON. A mismatch means
user-visible output changed. Regenerate with
`UPDATE_GOLDEN=1 cargo test --test golden` only when the change is intended,
and commit the golden diff with the reason.

`crates/idx-mcp/tests/mcp.rs` drives `idx-mcp` over stdio JSON-RPC with the
mock provider. `crates/idx-mcp/tests/golden/tools.json` pins what agents see (tool
names, descriptions, input schemas); regenerate it the same way after an
intended change.

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
nix develop --command scripts/live-smoke.sh --compare ./old/bin/idx  # A/B against an older build
scripts/live-smoke.sh --dry-run
```

- `stocks`: every `stocks` command for one ticker, plus `compare BUMI,ADRO,AIMS`,
  whose MSN key ratios contain non-finite values.
- `ownership`: `discover`, then imports the newest supported report into a temp
  DB and lists `releases`. IDX sits behind Cloudflare and returns `403` to many
  datacenter IPs, including GitHub-hosted runners; run it from a network IDX accepts.

`--compare <old-idx>` runs every case with both binaries back to back, each
with its own config and cache, and fails on a different exit code, any added,
missing, or retyped JSON key, or changed values for `profile`, `financials`,
`earnings`, and ownership results. Build the old side with, for example,
`nix build "git+file://$PWD?ref=main" -o old`.

Each run isolates config, cache, and data under `tmp/live-smoke/<timestamp>/`
and keeps per-case `.stdout`, `.stderr`, and `.validation` logs there.

`scripts/audit-msn-fundamentals.sh` is a heavier sweep of valuation output
across the whole IDX MSN symbol map, for provider-health audits.

## Agent smoke test

The MCP tests prove the protocol works; they cannot show whether a real
coding agent picks the right tools. `tests/agents/agent-smoke.sh` runs each
case in `tests/agents/cases.json` through Claude Code, Codex, pi, Hermes Agent,
and opencode in headless mode against a local `idx-mcp` build, then checks
the tools each agent called (and their key arguments) and, with mock data, key
facts in the answer. It is opt-in: it needs agent logins and model credits,
and agent behavior varies between runs, so it is never part of the gate.

```bash
nix develop .#agents --command tests/agents/agent-smoke.sh            # every agent found
nix develop .#agents --command tests/agents/agent-smoke.sh --agents claude,codex --runs 3
tests/agents/agent-smoke.sh --live --case shareholders --keep          # real data
```

- Each agent gets an isolated config that registers only the local server;
  your own agent settings and MCP servers are not loaded or changed. Logins
  are copied from the agent's home (Codex `auth.json`, pi and Hermes auth
  files) into a temp dir that is deleted afterwards; `--keep` keeps the run
  output but still deletes the copied credentials.
- Tools are auto-approved for the run (Claude `dontAsk` with
  `mcp__idx__*`, Codex `default_tools_approval_mode="approve"`); Claude and pi
  also get no built-in tools, so they can only answer through MCP.
- `IDX_AGENT_MODEL_<AGENT>` picks a model (for example
  `IDX_AGENT_MODEL_CLAUDE=sonnet`). `IDX_AGENT_EXEC_PREFIX` prefixes every
  agent command, for hosts that inject API keys through a wrapper.
- The run fails only when an agent fails a case in every run; use `--runs` to
  measure pass rates. `--keep` keeps raw agent output and `results.jsonl`.
- `nix develop .#agents` adds Claude Code, Codex, and opencode from nixpkgs
  after your PATH, so installed versions win. pi and Hermes Agent must be
  installed on the host.
