// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import starlightLlmsTxt from 'starlight-llms-txt';

export default defineConfig({
	site: 'https://idx.rasyidanaf.com',
	integrations: [
		starlight({
			title: 'idx',
			description:
				'Indonesian stock market (IDX) data and KSEI shareholder ownership, from the terminal or an AI agent through MCP.',
			logo: { light: './src/assets/wordmark-light.svg', dark: './src/assets/wordmark-dark.svg', alt: 'idx', replacesTitle: true },
			favicon: '/favicon.svg',
			social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/0xrsydn/idx-cli' }],
			editLink: { baseUrl: 'https://github.com/0xrsydn/idx-cli/edit/main/web/' },
			customCss: [
				'@fontsource/ibm-plex-sans/400.css',
				'@fontsource/ibm-plex-sans/600.css',
				'@fontsource/jetbrains-mono/400.css',
				'./src/styles/theme.css',
			],
			routeMiddleware: './src/routeData.ts',
			components: {
				PageTitle: './src/components/PageTitle.astro',
			},
			sidebar: [
				{
					label: 'Start',
					items: ['start/install', 'start/quickstart', 'start/config'],
				},
				{
					label: 'CLI',
					items: ['cli/stocks', 'cli/ownership', 'cli/output', 'cli/reference'],
				},
				{
					label: 'MCP server',
					items: [
						{ label: 'Overview', slug: 'mcp' },
						'mcp/setup',
						'mcp/tools',
					],
				},
				{
					label: 'For agents',
					items: [
						{ label: 'Using idx from an agent', slug: 'agents' },
						'agents/install',
						{ label: 'llms.txt', link: '/llms.txt' },
					],
				},
				'about',
			],
			plugins: [
				starlightLlmsTxt({
					projectName: 'idx-cli',
					details: [
						'- `idx` is a CLI; `idx-mcp` is an MCP server over the same library. Both read the same config, cache and ownership database.',
						'- Output is JSON whenever stdout is not a terminal. Errors are one JSON object on stderr; exit codes: 2 invalid input, 3 not found, 4 upstream unavailable, 5 unsupported, 1 other.',
						'- Every page is also available as Markdown by appending `.md` to its path, for example `/agents/install.md`.',
					].join('\n'),
				}),
			],
		}),
	],
});
