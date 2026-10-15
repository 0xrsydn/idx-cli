import { defineRouteMiddleware } from '@astrojs/starlight/route-data';

// Point each docs page's og:image at its card from src/pages/og/[...slug].png.ts.
export const onRequest = defineRouteMiddleware((context) => {
	const { entry, head } = context.locals.starlightRoute;
	const image = new URL(`/og/${entry.id}.png`, context.site);
	head.push(
		{ tag: 'meta', attrs: { property: 'og:image', content: image.href } },
		{ tag: 'meta', attrs: { property: 'og:image:width', content: '1200' } },
		{ tag: 'meta', attrs: { property: 'og:image:height', content: '630' } },
		{ tag: 'meta', attrs: { property: 'og:image:alt', content: `${entry.data.title} | idx` } },
	);
});
