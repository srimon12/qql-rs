import type { APIRoute, GetStaticPaths } from "astro";
import { getCollection } from "astro:content";
import sharp from "sharp";
import { SITE } from "../../config/site";

function escapeXml(unsafe: string): string {
  return unsafe
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&apos;");
}

function wrapText(text: string, maxCharsPerLine: number, maxLines: number): string[] {
  const words = text.trim().split(/\s+/);
  const lines: string[] = [];
  let currentLine = "";

  for (const word of words) {
    if ((currentLine + " " + word).trim().length <= maxCharsPerLine) {
      currentLine = (currentLine + " " + word).trim();
    } else {
      if (currentLine) {
        lines.push(currentLine);
      }
      currentLine = word;
      if (lines.length >= maxLines - 1) {
        break;
      }
    }
  }
  if (currentLine && lines.length < maxLines) {
    lines.push(currentLine);
  }

  if (lines.length === maxLines && words.length > 0) {
    const lastLine = lines[maxLines - 1];
    if (text.length > lines.join(" ").length) {
      lines[maxLines - 1] = lastLine.replace(/[.,;:]?$/, "") + "…";
    }
  }

  return lines;
}

function getCategoryFromSlug(slug: string): string {
  if (slug === "home" || slug === "og-image") return "DECLARATIVE VECTOR SEARCH";
  if (slug === "playground") return "INTERACTIVE PLAYGROUND";
  if (slug.includes("getting-started")) return "GETTING STARTED";
  if (slug.includes("guides")) return "PRODUCTION GUIDE";
  if (slug.includes("language")) return "LANGUAGE REFERENCE";
  if (slug.includes("edge")) return "IN-PROCESS EDGE RUNTIME";
  if (slug.includes("sdks")) return "NATIVE SDKs";
  if (slug.includes("tools")) return "DEVELOPER TOOLING";
  if (slug.includes("reference")) return "API & ERROR SPECIFICATION";
  if (slug.includes("contributing")) return "CONTRIBUTING";
  return "DOCUMENTATION";
}

function generateSvg({
  title,
  description,
  category,
}: {
  title: string;
  description: string;
  category: string;
}): string {
  const titleLines = wrapText(title, 34, 2);
  const descLines = wrapText(description, 58, 3);

  const titleTspans = titleLines
    .map(
      (line, i) =>
        `<tspan x="80" y="${250 + i * 56}" font-weight="700">${escapeXml(line)}</tspan>`,
    )
    .join("\n");

  const descStartY = 250 + titleLines.length * 56 + 18;
  const descTspans = descLines
    .map(
      (line, i) =>
        `<tspan x="80" y="${descStartY + i * 32}">${escapeXml(line)}</tspan>`,
    )
    .join("\n");

  return `
<svg width="1200" height="630" viewBox="0 0 1200 630" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="bgGrad" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#181816"/>
      <stop offset="50%" stop-color="#141413"/>
      <stop offset="100%" stop-color="#0f0f0e"/>
    </linearGradient>
    <radialGradient id="glowGrad" cx="0.8" cy="0.2" r="0.6">
      <stop offset="0%" stop-color="#d96b43" stop-opacity="0.18"/>
      <stop offset="60%" stop-color="#d96b43" stop-opacity="0.03"/>
      <stop offset="100%" stop-color="#d96b43" stop-opacity="0"/>
    </radialGradient>
    <radialGradient id="glowGrad2" cx="0.15" cy="0.85" r="0.5">
      <stop offset="0%" stop-color="#ba5442" stop-opacity="0.08"/>
      <stop offset="100%" stop-color="#ba5442" stop-opacity="0"/>
    </radialGradient>
    <pattern id="grid" width="36" height="36" patternUnits="userSpaceOnUse">
      <path d="M 36 0 L 0 0 0 36" fill="none" stroke="#262623" stroke-width="1" stroke-opacity="0.6"/>
    </pattern>
  </defs>

  <!-- Background -->
  <rect width="1200" height="630" fill="url(#bgGrad)"/>
  <rect width="1200" height="630" fill="url(#grid)" opacity="0.8"/>
  <rect width="1200" height="630" fill="url(#glowGrad)"/>
  <rect width="1200" height="630" fill="url(#glowGrad2)"/>

  <!-- Outer frame border -->
  <rect x="36" y="36" width="1128" height="558" rx="16" fill="none" stroke="#2e2e2a" stroke-width="1.5"/>

  <!-- Top bar -->
  <g transform="translate(80, 80)">
    <!-- Veristamp attestation ring -->
    <circle cx="18" cy="18" r="14.22" fill="none" stroke="#b04930" stroke-width="2.88"/>
    <path fill="#f4efe6" d="M7.535 11.248 L18 30.084 L28.465 11.248 L24.5 9.045 L18 20.744 L11.5 9.045 Z"/>
    <text x="48" y="25" font-family="DejaVu Serif, Georgia, serif" font-size="24" font-weight="700" fill="#f5f4ed" letter-spacing="-0.5">QQL</text>
    <text x="108" y="24" font-family="DejaVu Sans, Arial, sans-serif" font-size="13" font-weight="500" fill="#78716c" letter-spacing="0.5">/ ${escapeXml(SITE.org)}</text>
    
    <!-- Category Badge -->
    <rect x="740" y="2" width="300" height="30" rx="6" fill="#1c1c1a" stroke="#383834" stroke-width="1"/>
    <text x="890" y="21" font-family="DejaVu Sans Mono, monospace" font-size="11" font-weight="700" fill="#d96b43" letter-spacing="1.2" text-anchor="middle">${escapeXml(category)}</text>
  </g>

  <!-- Title & Description -->
  <g>
    <text font-family="DejaVu Serif, Georgia, serif" font-size="46" fill="#f5f4ed" letter-spacing="-0.8">
      ${titleTspans}
    </text>
    <text font-family="DejaVu Sans, Arial, sans-serif" font-size="20" fill="#a8a29e" letter-spacing="-0.2">
      ${descTspans}
    </text>
  </g>

  <!-- Bottom status bar -->
  <g transform="translate(80, 535)">
    <line x1="0" y1="0" x2="1040" y2="0" stroke="#2a2a26" stroke-width="1"/>
    <text x="0" y="28" font-family="DejaVu Sans Mono, monospace" font-size="13" font-weight="600" fill="#f5f4ed" letter-spacing="0.5">qql.veristamp.in</text>
    <circle cx="160" cy="24" r="2.5" fill="#52524e"/>
    <text x="175" y="28" font-family="DejaVu Sans, Arial, sans-serif" font-size="13" fill="#78716c">SQL for Qdrant vector search</text>
    <text x="1040" y="28" font-family="DejaVu Sans Mono, monospace" font-size="12" fill="#d96b43" text-anchor="end">Rust, Python, Node, WASM</text>
  </g>
</svg>`.trim();
}

export const getStaticPaths: GetStaticPaths = async () => {
  const docs = await getCollection("docs");

  const docPaths = docs.map((entry) => {
    const rawSlug = entry.id.replace(/\.(mdoc|md)$/, "").replace(/^\//, "");
    return {
      params: { slug: rawSlug },
      props: {
        title: entry.data.title,
        description: entry.data.description || "Declarative vector search for Qdrant.",
        category: getCategoryFromSlug(rawSlug),
      },
    };
  });

  const specialPaths = [
    {
      params: { slug: "home" },
      props: {
        title: "QQL: SQL for Qdrant Vector Search",
        description:
          "One typed declarative query language for hybrid search, filtering, mutations, multitenancy, and schema across Rust, Python, Node.js, WASM, REST, gRPC, and edge.",
        category: "DECLARATIVE VECTOR SEARCH",
      },
    },
    {
      params: { slug: "og-image" },
      props: {
        title: "QQL: SQL for Qdrant Vector Search",
        description:
          "One typed declarative query language for hybrid search, filtering, mutations, multitenancy, and schema across Rust, Python, Node.js, WASM, REST, gRPC, and edge.",
        category: "DECLARATIVE VECTOR SEARCH",
      },
    },
    {
      params: { slug: "playground" },
      props: {
        title: "QQL Playground: in-browser WASM parser and planner",
        description:
          "Interactive browser playground for QQL. Parse queries, inspect ASTs, verify planned execution routes, and test AST filter injection in real time.",
        category: "INTERACTIVE PLAYGROUND",
      },
    },
  ];

  return [...specialPaths, ...docPaths];
};

export const GET: APIRoute = async ({ props }) => {
  const { title, description, category } = props as {
    title: string;
    description: string;
    category: string;
  };

  const svg = generateSvg({ title, description, category });
  const pngBuffer = await sharp(Buffer.from(svg)).png({ quality: 90 }).toBuffer();

  return new Response(pngBuffer, {
    headers: {
      "Content-Type": "image/png",
      "Cache-Control": "public, max-age=31536000, immutable",
    },
  });
};
