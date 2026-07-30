#!/usr/bin/env node
/**
 * website/dist + playground dist → dist-site/
 * Fails if either side is missing.
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");
const WEBSITE = process.env.WEBSITE_DIST || path.join(ROOT, "website/dist");
const PLAY = process.env.PLAYGROUND_DIST || path.join(ROOT, ".playground-dist");
const OUT = process.env.OUT_DIST || path.join(ROOT, "dist-site");

function copy(src, dest) {
  fs.mkdirSync(dest, { recursive: true });
  for (const e of fs.readdirSync(src, { withFileTypes: true })) {
    const s = path.join(src, e.name);
    const d = path.join(dest, e.name);
    e.isDirectory() ? copy(s, d) : fs.copyFileSync(s, d);
  }
}

function need(file, label) {
  if (!fs.existsSync(file)) {
    console.error(`missing ${label}: ${file}`);
    process.exit(1);
  }
}

need(path.join(WEBSITE, "index.html"), "website dist");
need(path.join(PLAY, "index.html"), "playground dist");

if (fs.existsSync(OUT)) fs.rmSync(OUT, { recursive: true });
copy(WEBSITE, OUT);
copy(PLAY, path.join(OUT, "playground"));

// headers: site + playground (prefix playground paths)
const parts = [];
const wh = path.join(WEBSITE, "_headers");
const ph = path.join(PLAY, "_headers");
if (fs.existsSync(wh)) parts.push(fs.readFileSync(wh, "utf8").trim());
if (fs.existsSync(ph)) {
  parts.push(
    fs
      .readFileSync(ph, "utf8")
      .split("\n")
      .map((line) => {
        if (!line || line.startsWith("#") || /^\s/.test(line)) return line;
        if (line.startsWith("/playground")) return line;
        return line.startsWith("/") ? `/playground${line === "/*" ? "/*" : line}` : line;
      })
      .join("\n")
      .trim(),
  );
}
if (parts.length) fs.writeFileSync(path.join(OUT, "_headers"), parts.join("\n\n") + "\n");

fs.writeFileSync(
  path.join(OUT, "_redirects"),
  [
    "/playground  /playground/  301",
    "/playground/*  /playground/index.html  200",
    "/docs  /docs/  301",
    "",
  ].join("\n"),
);

need(path.join(OUT, "playground/index.html"), "merged playground");
console.log("ok →", OUT);
