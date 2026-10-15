// Serves every docs page as plain Markdown at `<path>.md`, so agents can read
// the docs without HTML (for example /agents/install.md). MDX-only syntax is
// rewritten into Markdown.
import type { APIRoute, GetStaticPaths } from 'astro';
import { getCollection } from 'astro:content';
import { mcpClientsMarkdown } from '../data/mcp-clients';

export const getStaticPaths: GetStaticPaths = async () => {
	const docs = await getCollection('docs');
	return docs.map((entry) => ({ params: { slug: entry.id }, props: { entry } }));
};

function toPlainMarkdown(title: string, body: string): string {
	const text = body
		.replace(/<!--[\s\S]*?-->\n*/g, '')
		.replace(/^import .*\n+/gm, '')
		.replace(/^<McpClientTabs \/>$/gm, () => mcpClientsMarkdown())
		// <Tabs>: each <TabItem label="X"> becomes a bold label.
		.replace(/^<\/?Tabs[^>]*>\n*/gm, '')
		.replace(/^<TabItem label="([^"]+)">$/gm, '**$1**')
		.replace(/^<\/TabItem>\n*/gm, '')
		// Asides (:::tip[Title] ... :::) become blockquote-free sections.
		.replace(/^:::\w+\[([^\]]+)\]$/gm, '**$1**\n')
		.replace(/^:::\w+$/gm, '')
		.replace(/^:::$/gm, '')
		.replace(/^```(\w+) wrap$/gm, '```$1')
		.replace(/\n{3,}/g, '\n\n')
		.trim();
	return `# ${title}\n\n${text}\n`;
}

export const GET: APIRoute = ({ props }) => {
	const { entry } = props;
	return new Response(toPlainMarkdown(entry.data.title, entry.body ?? ''), {
		headers: { 'Content-Type': 'text/markdown; charset=utf-8' },
	});
};
