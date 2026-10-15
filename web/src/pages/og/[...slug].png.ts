import type { APIRoute, GetStaticPaths } from 'astro';
import { getCollection } from 'astro:content';
import { docsCard } from '../../og/cards';

// Sidebar group labels, by the first segment of the page's path.
const sections: Record<string, string> = {
	start: 'Start',
	cli: 'CLI',
	mcp: 'MCP server',
	agents: 'For agents',
	about: 'About',
};

export const getStaticPaths = (async () => {
	const pages = await getCollection('docs');
	return pages.map((page) => ({
		params: { slug: page.id },
		props: {
			section: sections[page.id.split('/')[0]] ?? 'Docs',
			title: page.data.title,
			description: page.data.description,
			path: `/${page.id}/`,
		},
	}));
}) satisfies GetStaticPaths;

export const GET: APIRoute = async ({ props }) =>
	new Response(await docsCard(props as Parameters<typeof docsCard>[0]), { headers: { 'Content-Type': 'image/png' } });
