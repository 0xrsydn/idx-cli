// Open Graph cards (1200x630 PNG), rendered at build time. satori lays out the
// card and turns its text into SVG paths, so sharp rasterizes it without any
// system fonts.
import satori from 'satori';
import sharp from 'sharp';
import plex400 from '@fontsource/ibm-plex-sans/files/ibm-plex-sans-latin-400-normal.woff?inline';
import plex600 from '@fontsource/ibm-plex-sans/files/ibm-plex-sans-latin-600-normal.woff?inline';
import mono400 from '@fontsource/jetbrains-mono/files/jetbrains-mono-latin-400-normal.woff?inline';
import mono700 from '@fontsource/jetbrains-mono/files/jetbrains-mono-latin-700-normal.woff?inline';

const W = 1200;
const H = 630;
const PAD = 72;

const c = {
	bg: '#0d0e10',
	surface: '#16181c',
	line: '#2c3036',
	text: '#e8eaed',
	soft: '#c3c8cf',
	muted: '#9aa1ab',
	dim: '#6f7782',
	accent: '#ce1126',
	link: '#ff6b6f',
};

const sans = 'IBM Plex Sans';
const mono = 'JetBrains Mono';

const font = (dataUrl: string) => Buffer.from(dataUrl.slice(dataUrl.indexOf(',') + 1), 'base64');
const fonts = [
	{ name: sans, data: font(plex400), weight: 400 as const },
	{ name: sans, data: font(plex600), weight: 600 as const },
	{ name: mono, data: font(mono400), weight: 400 as const },
	{ name: mono, data: font(mono700), weight: 700 as const },
];

type Style = Record<string, string | number>;
type Node = { type: string; props: { style?: Style; children?: Child | Child[] } };
type Child = Node | string;

const h = (type: string, style: Style, ...children: Child[]): Node => ({
	type,
	props: { style: { display: 'flex', ...style }, children },
});

async function png(root: Node): Promise<Buffer> {
	const svg = await satori(root as never, { width: W, height: H, fonts });
	return sharp(Buffer.from(svg)).png().toBuffer();
}

const wordmark = h(
	'div',
	{ alignItems: 'center', gap: 4, fontFamily: mono, fontWeight: 700, fontSize: 40, color: c.text },
	'idx',
	h('div', { width: 17, height: 36, background: c.accent }),
);

const footer = (left: string, right?: string) =>
	h(
		'div',
		{ justifyContent: 'space-between', fontFamily: mono, fontSize: 22, color: c.dim },
		h('div', {}, left),
		h('div', {}, right ?? ''),
	);

const frame = (...children: Child[]) =>
	h(
		'div',
		{ width: W, height: H, background: c.bg, color: c.text },
		h('div', { position: 'absolute', top: 64, right: PAD, bottom: 52, left: PAD, flexDirection: 'column' }, ...children),
	);

// BBCA from the 2026-09-30 KSEI snapshot, the same capture as the landing page's
// ownership spotlight (src/pages/index.astro). Refresh both together.
const spotlight = {
	ticker: 'BBCA',
	snapshot: '2026-09-30',
	top: { name: 'DWIMURIA INVESTAMA ANDALAN', pct: 54.94 },
	others: [1.15, 1.07, 1.02],
	freeFloat: 41.82,
};

/** The site card: the tagline over BBCA's ownership split. */
export function siteCard(): Promise<Buffer> {
	const s = spotlight;
	const othersPct = s.others.reduce((a, b) => a + b, 0);
	const gap = 3;
	const barW = W - 2 * PAD - gap * (s.others.length + 1);
	const seg = (pct: number) => (barW * pct) / 100;
	const otherShades = [c.soft, c.muted, c.dim];
	const label = (title: string, value: string, style: Style = {}) =>
		h(
			'div',
			{ flexDirection: 'column', gap: 4, fontFamily: mono, ...style },
			h('div', { fontSize: 20, color: c.muted }, title),
			h('div', { fontSize: 28, fontWeight: 700, color: c.text }, value),
		);

	return png(
		frame(
			h('div', { justifyContent: 'space-between', alignItems: 'center' }, wordmark, h('div', { fontFamily: mono, fontSize: 20, color: c.dim }, `KSEI snapshot ${s.snapshot}`)),
			h(
				'div',
				{ flexDirection: 'column', marginTop: 36, fontFamily: sans, fontWeight: 600, fontSize: 52, lineHeight: 1.18 },
				h('div', {}, 'Indonesian stocks and ownership,'),
				h('div', { color: c.muted }, 'in your terminal and your agent.'),
			),
			h('div', { marginTop: 34, fontFamily: mono, fontSize: 24, color: c.soft }, h('span', { color: c.dim, marginRight: 14 }, '$'), `idx ownership ticker ${s.ticker}`),
			h(
				'div',
				{ marginTop: 18, height: 68, gap },
				h('div', { width: seg(s.top.pct), background: c.accent }),
				...s.others.map((pct, i) => h('div', { width: seg(pct), background: otherShades[i] })),
				h('div', {
					width: seg(s.freeFloat),
					background: c.surface,
					backgroundImage: `repeating-linear-gradient(45deg, ${c.line} 0px, ${c.line} 3px, ${c.surface} 3px, ${c.surface} 10px)`,
				}),
			),
			h(
				'div',
				{ marginTop: 18, position: 'relative', height: 72 },
				label(s.top.name, `${s.top.pct.toFixed(2)}%`),
				label(`+${s.others.length} holders`, `${othersPct.toFixed(2)}%`, { position: 'absolute', left: seg(s.top.pct) + gap }),
				label('FREE FLOAT', `${s.freeFloat.toFixed(2)}%`, { position: 'absolute', right: 0, alignItems: 'flex-end' }),
			),
			h('div', { flexGrow: 1 }),
			footer('idx.rasyidanaf.com', 'CLI · MCP server · MIT'),
		),
	);
}

/** A docs page card: section, title and description. */
export function docsCard(page: { section: string; title: string; description?: string; path: string }): Promise<Buffer> {
	return png(
		frame(
			wordmark,
			h('div', { flexGrow: 1 }),
			h('div', { fontFamily: mono, fontSize: 24, letterSpacing: 2, color: c.link }, page.section.toUpperCase()),
			h('div', { marginTop: 18, fontFamily: sans, fontWeight: 600, fontSize: page.title.length > 22 ? 64 : 76, lineHeight: 1.1, maxWidth: 1000 }, page.title),
			...(page.description
				? [h('div', { marginTop: 24, fontFamily: sans, fontSize: 32, lineHeight: 1.35, color: c.muted, maxWidth: 1000 }, page.description)]
				: []),
			h('div', { flexGrow: 1 }),
			h('div', { height: 2, background: c.line, marginBottom: 26 }),
			footer(`idx.rasyidanaf.com${page.path}`),
		),
	);
}
