import { spawnSync } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const websiteRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const workspaceRoot = resolve(websiteRoot, "..");
const output = join(websiteRoot, ".wasm", "qql-wasm");
const cache = join(websiteRoot, "node_modules", ".cache", "qql-docs-wasm-pack");

rmSync(output, { recursive: true, force: true });
mkdirSync(dirname(output), { recursive: true });

const build = spawnSync(
	"wasm-pack",
	[
		"build",
		join(workspaceRoot, "crates", "qql-wasm"),
		"--release",
		"--target",
		"web",
		"--out-dir",
		output,
	],
	{
		cwd: workspaceRoot,
		encoding: "utf8",
		stdio: "inherit",
		env: {
			...process.env,
			RUSTC_WRAPPER: "",
			WASM_PACK_CACHE: cache,
		},
	},
);

if (build.status !== 0) {
	process.exit(build.status ?? 1);
}
