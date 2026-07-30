/** QQL product site — qql.veristamp.in (local copy of Veristamp config surface). */

export const DOMAIN = "qql.veristamp.in";

export const SITE = {
  name: "QQL",
  org: "Veristamp",
  domain: DOMAIN,
  url: `https://${DOMAIN}`,
  email: "support@veristamp.in",
} as const;

export const APPS = {
  home: {
    name: "QQL",
    url: `https://${DOMAIN}`,
    basePath: "/",
  },
  docs: {
    name: "QQL Docs",
    url: `https://${DOMAIN}/docs`,
    basePath: "/docs",
  },
  playground: {
    name: "QQL Playground",
    url: `https://${DOMAIN}/playground`,
    basePath: "/playground",
  },
  github: {
    name: "qql-rs",
    url: "https://github.com/srimon12/qql-rs",
  },
  blog: {
    url: "https://veristamp.in/blog",
  },
} as const;

export const SOCIAL = {
  twitter: "https://x.com/srimon_12",
  github: "https://github.com/srimon12/qql-rs",
  orgGithub: "https://github.com/srimon12",
  linkedin: "https://linkedin.com/in/srimon12",
} as const;

/** Optional analytics — leave empty to disable */
export const ANALYTICS = {
  url: "https://stats.veristamp.in",
  id: "", // set when ready
} as const;

export const ROBOTS_AI_AGENTS = [
  "GPTBot",
  "ChatGPT-User",
  "ClaudeBot",
  "Claude-Web",
  "anthropic-ai",
  "Google-Extended",
  "PerplexityBot",
  "Amazonbot",
  "Applebot-Extended",
  "Bytespider",
  "CCBot",
  "meta-externalagent",
] as const;

export function generateRobotsTxt({
  sitemapUrl,
  disallow = [],
}: {
  sitemapUrl: string;
  disallow?: string[];
}) {
  const rules = [
    "User-agent: *",
    "Allow: /",
    ...disallow.map((d) => `Disallow: ${d}`),
    "",
    ...ROBOTS_AI_AGENTS.flatMap((agent) => [
      `User-agent: ${agent}`,
      "Allow: /",
      "",
    ]),
    `Sitemap: ${sitemapUrl}`,
  ];
  return rules.join("\n");
}
