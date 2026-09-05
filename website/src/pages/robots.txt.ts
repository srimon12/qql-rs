import type { APIRoute } from "astro";
import { APPS, generateRobotsTxt } from "../config/site";

export const GET: APIRoute = () => {
	const content = generateRobotsTxt({
		sitemapUrl: `${APPS.home.url}/sitemap-index.xml`,
	});

	return new Response(content, {
		headers: { "Content-Type": "text/plain" },
	});
};
