// Serves every docs page as raw Markdown at `<path>.md`, so agents can read
// the docs without HTML (for example /agents/install.md).
import type { APIRoute, GetStaticPaths } from 'astro';
import { getCollection } from 'astro:content';

export const getStaticPaths: GetStaticPaths = async () => {
	const docs = await getCollection('docs');
	return docs.map((entry) => ({ params: { slug: entry.id }, props: { entry } }));
};

export const GET: APIRoute = ({ props }) => {
	const { entry } = props;
	const body = `# ${entry.data.title}\n\n${entry.body ?? ''}`.replace(/<!--[\s\S]*?-->\n*/g, '').replace(/^import .*\n+/gm, '');
	return new Response(body, { headers: { 'Content-Type': 'text/markdown; charset=utf-8' } });
};
