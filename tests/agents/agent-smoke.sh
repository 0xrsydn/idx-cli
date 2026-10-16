#!/usr/bin/env bash
# Agent smoke test: runs real coding agents headless against a local idx-mcp
# build and checks which MCP tools they call. Opt-in: it needs agent logins
# and model credits, and agent behavior is nondeterministic. It is not part
# of `nix flake check`. See docs/SMOKE.md.
set -Eeuo pipefail

usage() {
    cat <<'EOF'
Usage: tests/agents/agent-smoke.sh [options]

Runs each case in tests/agents/cases.json through each agent, with an
isolated agent config that registers only the local idx-mcp, and checks the
tools called (and, with mock data, key facts in the answer).

Options:
  --agents <list>   Comma-separated: claude,codex,pi,hermes,opencode
                    (default: every one found on PATH)
  --case <id>       Run only this case (repeatable)
  --runs <n>        Runs per agent and case (default: 1)
  --live            Real upstream data and the published ownership snapshot
                    instead of mock fixtures; answer text is not checked
  --npx <spec>      Register `npx -y <spec>` as users do, instead of the local
                    build: `idx-mcp` (published) or an `npm pack` tarball.
                    Runs the release binary, so it implies --live
  --keep            Keep the work directory (raw agent output, results.jsonl)
  --help            Show this help

Environment:
  IDX_AGENT_EXEC_PREFIX   Command prefix for every agent, e.g. a secrets
                          wrapper: "with-ai-secrets ai-tools"
  IDX_AGENT_MODEL_<AGENT> Model override, e.g. IDX_AGENT_MODEL_CLAUDE=sonnet,
                          IDX_AGENT_MODEL_OPENCODE=opencode-go/glm-5.3
  IDX_AGENT_TIMEOUT       Seconds per agent run (default: 300)
  IDX_BINARY_URL          With --npx: binary the npm package installs instead
                          of the release download (see docs/NPM_DISTRIBUTION.md)

Exit status: 0 when every agent passed every case at least once, 1 when some
agent failed a case in all runs, 2 on usage or setup errors.
EOF
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cases_file="$repo_root/tests/agents/cases.json"
all_agents=(claude codex pi hermes opencode)
agents=()
only_cases=()
runs=1
live=0
keep=0
npx_spec=
timeout_secs="${IDX_AGENT_TIMEOUT:-300}"
read -r -a exec_prefix <<< "${IDX_AGENT_EXEC_PREFIX:-}"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --agents) IFS=, read -r -a agents <<< "${2:?}"; shift 2 ;;
        --case) only_cases+=("${2:?}"); shift 2 ;;
        --runs) runs="${2:?}"; shift 2 ;;
        --live) live=1; shift ;;
        --npx) npx_spec="${2:?}"; live=1; shift 2 ;;
        --keep) keep=1; shift ;;
        --help|-h) usage; exit 0 ;;
        *) usage >&2; exit 2 ;;
    esac
done

[[ "$runs" =~ ^[1-9][0-9]*$ ]] || { echo "--runs must be a positive integer" >&2; exit 2; }
if ((BASH_VERSINFO[0] < 4 || (BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] < 4))); then
    echo "agent-smoke: bash 4.4+ is required (run it inside nix develop)" >&2
    exit 2
fi
for tool in jq cargo timeout ${npx_spec:+npx}; do
    command -v "$tool" >/dev/null || { echo "agent-smoke: $tool is required" >&2; exit 2; }
done

if [[ ${#agents[@]} -eq 0 ]]; then
    for agent in "${all_agents[@]}"; do
        command -v "$agent" >/dev/null && agents+=("$agent")
    done
fi
[[ ${#agents[@]} -gt 0 ]] || { echo "agent-smoke: no agent CLI found on PATH" >&2; exit 2; }
for agent in "${agents[@]}"; do
    [[ " ${all_agents[*]} " == *" $agent "* ]] || { echo "unknown agent: $agent" >&2; exit 2; }
    command -v "$agent" >/dev/null || { echo "agent-smoke: $agent not found on PATH" >&2; exit 2; }
done

work="$(mktemp -d "${TMPDIR:-/tmp}/idx-agent-smoke.XXXXXX")"
cleanup() {
    if [[ "$keep" == 1 ]]; then
        # Never leave copied agent logins or settings (which may hold API keys)
        # behind; each run's raw output is already under runs/.
        rm -rf -- "$work/codex" "$work/pi" "$work/hermes" "$work/opencode"
        echo "agent-smoke: kept $work (copied agent configs removed)"
    elif [[ "$work" == */idx-agent-smoke.* ]]; then
        rm -rf -- "$work"
    fi
}
trap cleanup EXIT

# --- idx-mcp under test ----------------------------------------------------

echo "agent-smoke: building idx-mcp ($([[ $live == 1 ]] && echo live || echo mock) data)"
features=()
[[ "$live" == 1 ]] || features=(--features idx-core/testing)
(cd "$repo_root" && cargo build -q --locked -p idx-cli -p idx-mcp "${features[@]}")
idx="$repo_root/target/debug/idx"

data_env=(
    "XDG_CONFIG_HOME=$work/idx/config"
    "XDG_CACHE_HOME=$work/idx/cache"
    "XDG_DATA_HOME=$work/idx/data"
)
if [[ "$live" == 1 ]]; then
    env "${data_env[@]}" "$idx" -o json ownership sync >/dev/null
else
    data_env+=(IDX_USE_MOCK_PROVIDER=1)
    fixtures="$repo_root/crates/idx-core/tests/fixtures"
    # The importer derives the release from the IDX file name.
    cp "$fixtures/ksei_above1_20260831_excerpt.xlsx" "$work/peng-2026-08-00017-satu-persen.xlsx"
    for file in "$work/peng-2026-08-00017-satu-persen.xlsx" \
        "$fixtures/ksei_balancepos_20260130_excerpt.txt" \
        "$fixtures/ksei_balancepos_20260227_excerpt.txt"; do
        env "${data_env[@]}" "$idx" -o json ownership import --file "$file" >/dev/null
    done
fi

# One server definition for every agent, as {command, args, env}.
if [[ -n "$npx_spec" ]]; then
    # What users configure, so each agent's own npx handling is exercised
    # (Hermes swaps in its managed Node). Each agent gets a fresh npm cache
    # below, so its first run is a cold install.
    server_env=("${data_env[@]}")
    [[ -z "${IDX_BINARY_URL:-}" ]] || server_env+=("IDX_BINARY_URL=$IDX_BINARY_URL")
    # npx runs a tarball only as a package to install: `npx -y -p file:<tgz> idx-mcp`.
    if [[ "$npx_spec" == *.tgz && ! -f "$npx_spec" ]]; then
        echo "agent-smoke: tarball not found: $npx_spec" >&2; exit 2
    fi
    npx_args=(-y "$npx_spec")
    [[ ! -f "$npx_spec" ]] || npx_args=(-y -p "file:$(realpath "$npx_spec")" idx-mcp)
    server_json="$(jq -nc --argjson args "$(printf '%s\n' "${npx_args[@]}" | jq -Rnc '[inputs]')" '{command: "npx", args: $args,
        env: ($ARGS.positional | map(capture("^(?<key>[^=]+)=(?<value>.*)$")) | from_entries)}' \
        --args "${server_env[@]}")"
else
    # A launcher with isolated data dirs, and the idx-core crate as working
    # directory (the mock provider reads tests/fixtures from there).
    launcher="$work/idx-mcp"
    {
        echo '#!/usr/bin/env bash'
        printf 'export %q\n' "${data_env[@]}"
        printf 'cd %q\n' "$repo_root/crates/idx-core"
        printf 'exec %q "$@"\n' "$repo_root/target/debug/idx-mcp"
    } > "$launcher"
    chmod +x "$launcher"
    server_json="$(jq -nc --arg cmd "$launcher" '{command: $cmd, args: [], env: {}}')"
fi

# --- per-agent setup, run, and tool-call extraction ---------------------------
# Each run_<agent> writes raw output to $out.raw, the answer to $out.answer, and
# one {"name","args"} JSON object per idx tool call to $out.calls, with names
# normalized to the bare tool name.

copy_if_present() { [[ -f "$1" ]] && cp "$1" "$2/"; return 0; }

setup_claude() {
    jq -n --argjson s "$server_json" '{mcpServers: {idx: ({type: "stdio"} + $s)}}' > "$work/claude-mcp.json"
}
run_claude() {
    local prompt="$1" out="$2" model=()
    [[ -n "${IDX_AGENT_MODEL_CLAUDE:-}" ]] && model=(--model "$IDX_AGENT_MODEL_CLAUDE")
    "${exec_prefix[@]}" claude -p "$prompt" "${model[@]}" \
        --mcp-config "$work/claude-mcp.json" --strict-mcp-config \
        --setting-sources project --no-session-persistence --tools "" \
        --permission-mode dontAsk --allowedTools "mcp__idx__*" \
        --output-format stream-json --verbose --max-turns 10 < /dev/null > "$out.raw"
    jq -c 'select(.type=="assistant") | .message.content[]? | select(.type=="tool_use")
           | select(.name | startswith("mcp__idx__"))
           | {name: (.name | ltrimstr("mcp__idx__")), args: .input}' "$out.raw" > "$out.calls"
    jq -r 'select(.type=="result") | .result // ""' "$out.raw" > "$out.answer"
}

setup_codex() {
    mkdir -p "$work/codex"
    copy_if_present "${CODEX_HOME:-$HOME/.codex}/auth.json" "$work/codex"
}
run_codex() {
    local prompt="$1" out="$2" model=() server=()
    [[ -n "${IDX_AGENT_MODEL_CODEX:-}" ]] && model=(-m "$IDX_AGENT_MODEL_CODEX")
    # -c values are TOML; JSON strings and arrays of them are valid TOML.
    mapfile -t server < <(jq -r '"mcp_servers.idx.command=\(.command | tojson)",
        "mcp_servers.idx.args=\(.args | tojson)",
        "mcp_servers.idx.env={\(.env | to_entries | map("\(.key)=\(.value | tojson)") | join(","))}"' <<< "$server_json")
    CODEX_HOME="$work/codex" "${exec_prefix[@]}" codex exec --json --ephemeral \
        --skip-git-repo-check -s read-only "${model[@]}" \
        -c "${server[0]}" -c "${server[1]}" -c "${server[2]}" \
        -c 'mcp_servers.idx.default_tools_approval_mode="approve"' \
        "$prompt" < /dev/null > "$out.raw"
    jq -c 'select(.type=="item.completed" and .item.type=="mcp_tool_call" and .item.server=="idx")
           | select(.item.status=="completed") | {name: .item.tool, args: .item.arguments}' "$out.raw" > "$out.calls"
    jq -r 'select(.type=="item.completed" and .item.type=="agent_message") | .item.text' "$out.raw" > "$out.answer"
}

setup_pi() {
    local src="${PI_CODING_AGENT_DIR:-$HOME/.pi/agent}"
    mkdir -p "$work/pi"
    for file in auth.json models.json settings.json; do copy_if_present "$src/$file" "$work/pi"; done
    # "direct" exposes each MCP tool by name (the default "codemode" wraps them).
    jq -n --argjson s "$server_json" '{mcpServers: {idx: ($s + {exposure: "direct"})}}' > "$work/pi/mcp.json"
}
run_pi() {
    local prompt="$1" out="$2" model=()
    [[ -n "${IDX_AGENT_MODEL_PI:-}" ]] && model=(--model "$IDX_AGENT_MODEL_PI")
    # No built-in tools, so the agent can only reach the data through MCP.
    PI_CODING_AGENT_DIR="$work/pi" "${exec_prefix[@]}" pi -p --mode json --no-session \
        --no-builtin-tools --no-skills --no-context-files "${model[@]}" "$prompt" < /dev/null > "$out.raw"
    jq -c 'select(.type=="tool_execution_start") | select(.toolName | startswith("mcp__idx__"))
           | {name: (.toolName | ltrimstr("mcp__idx__")), args: .args}' "$out.raw" > "$out.calls"
    jq -r 'select(.type=="agent_end") | .messages[-1].content[]? | select(.type=="text") | .text' "$out.raw" > "$out.answer"
}

setup_hermes() {
    local src="${HERMES_HOME:-$HOME/.hermes}"
    mkdir -p "$work/hermes"
    copy_if_present "$src/auth.json" "$work/hermes"
    copy_if_present "$src/.env" "$work/hermes"
    # Model providers can be user plugins. A symlink: cleanup removes only the link.
    [[ ! -d "$src/plugins" ]] || ln -s "$src/plugins" "$work/hermes/plugins"
    local server
    server="$(jq -c '. + {enabled: true}' <<< "$server_json")"
    if jq -e 'type == "object"' "$src/config.yaml" >/dev/null 2>&1; then
        # Hermes also writes config.yaml as JSON (e.g. in profiles).
        jq --argjson s "$server" '{model, mcp_servers: {idx: $s}} | del(.. | nulls)' \
            "$src/config.yaml" > "$work/hermes/config.yaml"
    else
        {
            # The top-level `model:` block, up to the next top-level key.
            [[ -f "$src/config.yaml" ]] && awk '/^[^[:space:]#]/ { in_model = /^model:/ } in_model' "$src/config.yaml"
            # JSON is valid YAML.
            printf 'mcp_servers:\n  idx: %s\n' "$server"
        } > "$work/hermes/config.yaml"
    fi
    # A bare `npx` runs on Hermes' own Node, fetched into a fresh HERMES_HOME
    # on first use (~20s); `hermes -z` waits only ~15s for MCP tools, so fetch
    # it first or the first case runs without them.
    if [[ -n "$npx_spec" ]]; then
        HERMES_HOME="$work/hermes" "${exec_prefix[@]}" hermes pm install npm >/dev/null \
            || { echo "agent-smoke: hermes pm install npm failed" >&2; exit 2; }
    fi
}
run_hermes() {
    local prompt="$1" out="$2" model=() session status=0
    [[ -n "${IDX_AGENT_MODEL_HERMES:-}" ]] && model=(-m "$IDX_AGENT_MODEL_HERMES")
    # Only the idx toolset: with its terminal, Hermes can debug and repair a
    # broken server mid-run and pass for the wrong reason.
    HERMES_HOME="$work/hermes" "${exec_prefix[@]}" hermes -z "$prompt" "${model[@]}" -t idx \
        --usage-file "$out.usage.json" < /dev/null > "$out.answer" || status=$?
    # Hermes reports a dead server only as "Connection closed"; its stderr is here.
    if [[ -f "$work/hermes/logs/mcp-stderr.log" ]]; then
        cp "$work/hermes/logs/mcp-stderr.log" "$out.mcp-stderr"
    fi
    ((status == 0)) || return "$status"
    # -z prints only the answer; tool calls come from the session export.
    session="$(jq -r '.session_id // empty' "$out.usage.json")"
    [[ -n "$session" ]] || return 1
    HERMES_HOME="$work/hermes" hermes sessions export --format jsonl --session-id "$session" \
        "$out.raw" --yes < /dev/null >/dev/null
    # Hermes may call MCP tools directly or through its tool_call wrapper.
    jq -c '.messages[]? | .tool_calls[]? | .function
           | (.arguments | (try fromjson catch {})) as $args
           | if .name == "tool_call" then ($args.calls[]? | {name, args: .arguments}) else {name, args: $args} end
           | select(.name | startswith("mcp__idx__"))
           | .name |= ltrimstr("mcp__idx__")' "$out.raw" > "$out.calls"
}

setup_opencode() {
    local version
    mkdir -p "$work/opencode/cwd"
    version="$("${exec_prefix[@]}" opencode --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -n1)"
    if [[ "${version%%.*}" -ge 2 ]]; then
        # OpenCode 2 (@opencode/cli). A denied call ends the run, and models
        # use Code Mode's `execute` while the server connects, so deny only
        # the tools that could route around it. --standalone keeps the run
        # off the shared background service.
        jq -n --argjson s "$server_json" \
            '{mcp: {servers: {idx: {type: "local", command: ([$s.command] + $s.args),
                                    environment: $s.env, codemode: false}}},
              permissions: ((["read", "edit", "glob", "grep", "shell", "subagent", "skill",
                              "question", "webfetch", "websearch", "external_directory"]
                             | map({action: ., resource: "*", effect: "deny"}))
                            + [{action: "idx_*", resource: "*", effect: "allow"}])}' \
            > "$work/opencode/cwd/opencode.json"
        echo --standalone > "$work/opencode/run-flag"
    else
        jq -n --argjson s "$server_json" \
            '{"$schema": "https://opencode.ai/config.json",
              mcp: {idx: {type: "local", command: ([$s.command] + $s.args), environment: $s.env, enabled: true}},
              permission: {"*": "deny", "idx_*": "allow"}}' \
            > "$work/opencode/cwd/opencode.json"
        echo --pure > "$work/opencode/run-flag"
    fi
}
run_opencode() {
    local prompt="$1" out="$2" model=() home="$work/opencode" flag
    [[ -n "${IDX_AGENT_MODEL_OPENCODE:-}" ]] && model=(-m "$IDX_AGENT_MODEL_OPENCODE")
    flag="$(< "$home/run-flag")"
    (cd "$home/cwd" && XDG_CONFIG_HOME="$home/config" XDG_DATA_HOME="$home/data" \
        XDG_CACHE_HOME="$home/cache" XDG_STATE_HOME="$home/state" \
        "${exec_prefix[@]}" opencode run "$flag" --format json "${model[@]}" "$prompt" < /dev/null > "$out.raw")
    jq -c 'select(.type=="tool_use") | .part | select(.tool | startswith("idx_"))
           | select(.state.status=="completed") | {name: (.tool | ltrimstr("idx_")), args: .state.input}' "$out.raw" > "$out.calls"
    jq -r 'select(.type=="text") | .part.text' "$out.raw" > "$out.answer"
}

# --- checks -----------------------------------------------------------------

# Prints failure reasons for one run; no output means it passed.
check_run() {
    local case_json="$1" out="$2"
    jq -rn --argjson case "$case_json" --slurpfile calls "$out.calls" '
        $case.expect_tools[] as $want
        | if $want.any then
            select([$calls[] | .name] | any(. as $n | $want.any | index($n)) | not)
            | "no call to any of \($want.any | join(", "))"
          else
            ([$calls[] | select(.name == $want.name)]) as $matches
            | if ($matches | length) == 0 then "no call to \($want.name)"
              elif ($want.args // {}) as $args | ($matches | any(.args | contains($args))) | not
              then "\($want.name) called without \($want.args | tojson): \($matches | map(.args) | tojson)"
              else empty end
          end' || echo "could not parse the agent's tool calls (see $out.raw)"
    if [[ "$live" == 0 ]]; then
        local pattern
        while IFS= read -r pattern; do
            # jq regexes (Oniguruma) support (?i) on every platform, unlike grep -P.
            jq -Rse --arg re "$pattern" 'test($re)' "$out.answer" >/dev/null \
                || echo "answer does not match /$pattern/"
        done < <(jq -r '.expect_text[]?' <<< "$case_json")
    fi
}

# --- run --------------------------------------------------------------------

mapfile -t case_ids < <(jq -r '.[].id' "$cases_file")
if [[ ${#only_cases[@]} -gt 0 ]]; then
    for id in "${only_cases[@]}"; do
        [[ " ${case_ids[*]} " == *" $id "* ]] || { echo "unknown case: $id" >&2; exit 2; }
    done
    case_ids=("${only_cases[@]}")
fi

results="$work/results.jsonl"
: > "$results"
echo "agent-smoke: agents=${agents[*]} cases=${#case_ids[@]} runs=$runs"
for agent in "${agents[@]}"; do
    if [[ -n "$npx_spec" ]]; then
        # A shared cache would let later agents reuse the first one's install.
        # Agents may filter env, so pass it explicitly.
        export npm_config_cache="$work/npm-cache/$agent"
        server_json="$(jq -c --arg cache "$npm_config_cache" '.env.npm_config_cache = $cache' <<< "$server_json")"
    fi
    "setup_$agent"
    for id in "${case_ids[@]}"; do
        # A case's "live" fields replace the mock ones, e.g. release dates.
        case_json="$(jq -c --arg id "$id" --argjson live "$live" '.[] | select(.id == $id)
            | if $live == 1 then . + (.live // {}) else . end | del(.live)' "$cases_file")"
        prompt="$(jq -r '.prompt' <<< "$case_json") Use the idx MCP tools."
        for ((run = 1; run <= runs; run++)); do
            out="$work/runs/$agent/$id.$run"
            mkdir -p "$(dirname "$out")"
            : > "$out.calls"; : > "$out.answer"
            started=$SECONDS
            if timeout "$timeout_secs" bash -c "set -eo pipefail; $(declare -f "run_$agent"); $(declare -p work server_json exec_prefix); run_$agent \"\$1\" \"\$2\"" _ "$prompt" "$out" 2> "$out.stderr"; then
                # Malformed agent output fails this run, not the whole matrix.
                failures="$(check_run "$case_json" "$out")" \
                    || failures="${failures:+$failures$'\n'}could not check the agent output (see $out.raw)"
            else
                status=$?
                # Name the cause here: without --keep the output is deleted.
                reason="$(grep -m1 -v '^[[:space:]]*$' "$out.answer" || tail -n1 "$out.stderr" || true)"
                failures="agent exited with status $status${reason:+: ${reason:0:300}}"
            fi
            status=pass
            [[ -z "$failures" ]] || status=fail
            jq -nc --arg agent "$agent" --arg case "$id" --argjson run "$run" --arg status "$status" \
                --arg failures "$failures" --argjson secs "$((SECONDS - started))" \
                --slurpfile calls "$out.calls" \
                '{agent: $agent, case: $case, run: $run, status: $status, secs: $secs,
                  tools: [$calls[] | .name], failures: ($failures | split("\n") | map(select(. != "")))}' >> "$results"
            printf '  %-9s %-13s run %d  %s  %3ss  %s\n' "$agent" "$id" "$run" "$status" "$((SECONDS - started))" \
                "$(jq -r '[.[] | .name] | join(",")' <(jq -s . "$out.calls"))"
            [[ -z "$failures" ]] || printf '      - %s\n' "${failures//$'\n'/$'\n      - '}"
        done
    done
done

echo
echo "agent-smoke: pass rate per agent and case"
jq -rs 'group_by(.agent)[] | "\(.[0].agent): " + (group_by(.case) | map("\(.[0].case) \(map(select(.status=="pass")) | length)/\(length)") | join("  "))' "$results"
consistent_failures="$(jq -s '[group_by([.agent, .case])[] | select(all(.status == "fail"))] | length' "$results")"
if [[ "$consistent_failures" -gt 0 ]]; then
    echo "agent-smoke: FAIL ($consistent_failures agent/case pairs failed every run)"
    exit 1
fi
echo "agent-smoke: PASS"
