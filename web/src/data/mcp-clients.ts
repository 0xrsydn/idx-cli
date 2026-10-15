// The one list of MCP client setups, used by the landing page, the MCP setup
// page and agents/install.md (and their .md output).

export interface McpClient {
	id: string;
	label: string;
	/** Where the config goes, shown above the snippet. */
	file: string;
	lang: string;
	code: string;
	/** Driven by tests/agents/agent-smoke.sh against the real agent. */
	smokeTested: boolean;
}

export const mcpClients: McpClient[] = [
	{ id: 'claude', label: 'Claude Code', file: 'Run in a terminal', lang: 'bash', smokeTested: true, code: 'claude mcp add idx -- npx -y idx-mcp' },
	{ id: 'codex', label: 'Codex', file: '~/.codex/config.toml', lang: 'toml', smokeTested: true, code: '[mcp_servers.idx]\ncommand = "npx"\nargs = ["-y", "idx-mcp"]' },
	{ id: 'cursor', label: 'Cursor', file: '~/.cursor/mcp.json (or .cursor/mcp.json in a project)', lang: 'json', smokeTested: false, code: '{\n  "mcpServers": {\n    "idx": { "command": "npx", "args": ["-y", "idx-mcp"] }\n  }\n}' },
	{ id: 'opencode', label: 'opencode', file: '~/.config/opencode/opencode.json (or opencode.json in a project)', lang: 'json', smokeTested: true, code: '{\n  "$schema": "https://opencode.ai/config.json",\n  "mcp": {\n    "idx": { "type": "local", "command": ["npx", "-y", "idx-mcp"], "enabled": true }\n  }\n}' },
	{ id: 'pi', label: 'pi', file: '~/.pi/agent/mcp.json', lang: 'json', smokeTested: true, code: '{\n  "mcpServers": {\n    "idx": { "command": "npx", "args": ["-y", "idx-mcp"], "exposure": "direct" }\n  }\n}' },
	{ id: 'hermes', label: 'Hermes', file: '~/.hermes/config.yaml', lang: 'yaml', smokeTested: true, code: 'mcp_servers:\n  idx:\n    command: npx\n    args: ["-y", "idx-mcp"]\n    enabled: true' },
	{ id: 'other', label: 'Other', file: 'Any MCP client that runs stdio servers', lang: 'text', smokeTested: false, code: 'command: npx\nargs: -y idx-mcp\n# or the command idx-mcp, if installed\n# add --allow-sync to expose ownership_sync' },
];

export const smokeTestedLabels = mcpClients.filter((c) => c.smokeTested).map((c) => c.label);

/** The setups as Markdown, for the .md output of pages that render <McpClientTabs />. */
export function mcpClientsMarkdown(): string {
	return mcpClients
		.map((c) => `### ${c.label}\n\n${c.file}:\n\n\`\`\`${c.lang}\n${c.code}\n\`\`\``)
		.join('\n\n');
}
