import type { APIRoute } from 'astro';
import { siteCard } from '../og/cards';

export const GET: APIRoute = async () => new Response(await siteCard(), { headers: { 'Content-Type': 'image/png' } });
