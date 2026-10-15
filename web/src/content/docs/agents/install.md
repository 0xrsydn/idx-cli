---
title: Agent-led install
description: Instructions for a coding agent to install idx and register idx-mcp with itself.
---

This page is written for a coding agent. A user sent you here to install
**idx** (a CLI for Indonesian stock market and KSEI ownership data) and
register its MCP server, **idx-mcp**, with the agent harness you are running
in. Follow the steps in order and report back at the end.

Ground rules:

- Never use `sudo`. Install into the user's home directory.
- Before you edit a config file outside the current project (anything under
  `~`), show the user the exact change and ask for confirmation.
- If a step fails, stop and tell the user what failed and the error, rather
  than trying unrelated workarounds.

## 1. Check the platform

Run `uname -sm`. Supported: Linux x86_64/aarch64 and macOS x86_64/arm64. On
Windows, stop and tell the user to use WSL.

## 2. Install idx

Use the first option that applies:

1. **Node.js is available** (`node --version` succeeds): nothing to install
   for the MCP server, which runs through `npx -y idx-mcp`. Install the CLI
   with `npm install --global idx-cli`; if that fails with a permissions
   error, skip it and use `npx -y idx-cli` in place of `idx` below.
2. **Otherwise**: run the install script, which verifies checksums and
   installs to `~/.local/bin`:

   ```bash
   curl -fsSL https://raw.githubusercontent.com/0xrsydn/idx-cli/main/install.sh | sh
   ```

   It installs only `idx`. For the MCP server without Node.js, use
   `cargo install idx-mcp` if `cargo` exists; otherwise tell the user that
   idx-mcp needs Node.js, Cargo or Nix, and continue with the CLI only.

Confirm with `idx version` (or `npx -y idx-cli version`).

## 3. Register idx-mcp with yourself

Work out which harness you are, then apply its setup. Use `npx` and
`["-y", "idx-mcp"]` as command and arguments, or `idx-mcp` with no arguments
if you installed the binary.

| Harness | How |
| --- | --- |
| Claude Code | run `claude mcp add idx -- npx -y idx-mcp` |
| Codex | add to `~/.codex/config.toml`: `[mcp_servers.idx]` with `command = "npx"`, `args = ["-y", "idx-mcp"]` |
| Cursor | add to `~/.cursor/mcp.json` under `mcpServers`: `"idx": {"command": "npx", "args": ["-y", "idx-mcp"]}` |
| opencode | add to `~/.config/opencode/opencode.json` under `mcp`: `"idx": {"type": "local", "command": ["npx", "-y", "idx-mcp"], "enabled": true}` |
| pi | add to `~/.pi/agent/mcp.json` under `mcpServers`: `"idx": {"command": "npx", "args": ["-y", "idx-mcp"], "exposure": "direct"}` |
| Hermes | add to `~/.hermes/config.yaml` under `mcp_servers`: `idx` with `command: npx`, `args: ["-y", "idx-mcp"]`, `enabled: true` |
| anything else | find how your harness registers a stdio MCP server, and ask the user if unsure |

Merge into existing files; never overwrite other servers or settings. Full
examples: [MCP setup](/mcp/setup/).

## 4. Download ownership data

```bash
idx ownership sync
```

This downloads a few MB into `~/.local/share/idx/ownership.db`. It is safe to
rerun.

## 5. Verify

Run:

```bash
idx -o json ownership ticker BBCA
```

It should list holders, the first being DWIMURIA INVESTAMA ANDALAN. Most
harnesses load MCP servers at startup, so the `idx` tools usually appear only
after a restart. If yours already shows them, call `get_shareholders` with
`{"ticker": "BBCA"}` as well.

## 6. Report to the user

Tell the user, briefly:

- what you installed and how (npm, install script or Cargo), and the version;
- which config file you changed for idx-mcp, or why you didn't;
- the result of the verification (largest BBCA holder and its percentage);
- whether they need to restart the agent to use the idx tools;
- a next thing to try, such as asking "Who are BBCA's largest holders, and
  what changed last month?"
