import type { EditorView } from "@codemirror/view";
import type { CompiledRoute } from "qql-wasm-current";
import { selectedRoute } from "./analysis";
import { all, required } from "./dom";
import { pretty } from "./display";
import { store } from "./store";
import { toast } from "./toasts";
import type {
	ExportLanguage,
	PlaygroundSettings,
} from "../playground-types";

let exportEditor: EditorView | null = null;

function quotePython(source: string): string {
	return `qql = """${source.replace(/"""/g, '\\"\\"\\"')}"""`;
}

function quoteRust(source: string): string {
	const hashes = source.includes('"#') ? "##" : "#";
	return `r${hashes}"${source}"${hashes}`;
}

function exportCode(
	language: ExportLanguage,
	source: string,
	conn: PlaygroundSettings,
	route: CompiledRoute | null,
	statementCount: number,
): string {
	const url = JSON.stringify(conn.qdrantUrl);
	const pythonKey = conn.qdrantKey ? JSON.stringify(conn.qdrantKey) : "None";

	if (language === "python") {
		return `# pip install pyqql
from pyqql import Client

${quotePython(source)}
# execute() accepts a complete QQL script, including multiple statements.
client = Client(url=${url}, api_key=${pythonKey})
report = client.execute(qql)
print(report)`;
	}

	if (language === "node") {
		const apiKey = conn.qdrantKey
			? `,\n  apiKey: ${JSON.stringify(conn.qdrantKey)}`
			: "";
		return `// npm install @veristamp/nqql
import { Client } from "@veristamp/nqql";

const qql = ${JSON.stringify(source)};
const client = new Client({
  url: ${url}${apiKey}
});
const report = await client.execute(qql);
console.log(report);`;
	}

	if (language === "rust") {
		const rustKey = conn.qdrantKey
			? `Some(${JSON.stringify(conn.qdrantKey)}.to_owned())`
			: "None";
		return `// Cargo.toml: qql = "0.1", tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
use qql::executor::{Executor, OnError};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let executor = Executor::rest(${url}, ${rustKey})?;
    let report = executor
        .execute(${quoteRust(source)}, OnError::Stop)
        .await?;
    println!("{report:#?}");
    Ok(())
}`;
	}

	if (!route) return "A compiled route is required before exporting cURL.";
	const payload = pretty(route.payload).replace(/'/g, "'\\''");
	const header = conn.qdrantKey
		? ` \\\n  -H ${JSON.stringify(`api-key: ${conn.qdrantKey}`)}`
		: "";
	return `# Statement ${store.state.selectedStatement + 1} of ${statementCount} · compiled from the editor
curl -X ${route.method} ${JSON.stringify(`${conn.qdrantUrl}${route.path}`)} \\
  -H "content-type: application/json"${header} \\
  --data '${payload}'`;
}

const embedStatus = required<HTMLElement>("[data-embed-status]");

export function renderExport(): void {
	if (!exportEditor) return;
	const source = exportEditor.state.doc.toString();
	const statements = store.state.analysis?.result.statements_count ?? 1;
	required<HTMLElement>("[data-export-output]").textContent = exportCode(
		store.state.exportLanguage,
		source,
		store.settings,
		selectedRoute(store.state.analysis, store.state.selectedStatement),
		statements,
	);
	const echo = document.querySelector("[data-export-echo]");
	if (echo)
		echo.textContent =
			statements > 1
				? `Exporting statement ${store.state.selectedStatement + 1} of ${statements}`
				: "";
}

export function syncExportTabs(): void {
	const tabs = all<HTMLButtonElement>("[data-export-tab]");
	tabs.forEach((item) => {
		item.setAttribute(
			"aria-pressed",
			String(item.dataset.exportTab === store.state.exportLanguage),
		);
	});
}

export function setupExporter(editor: EditorView): void {
	exportEditor = editor;
	const tabs = all<HTMLButtonElement>("[data-export-tab]");
	for (const tab of tabs) {
		tab.addEventListener("click", () => {
			store.state.exportLanguage = tab.dataset.exportTab as ExportLanguage;
			syncExportTabs();
			renderExport();
		});
	}
	required("[data-copy-export]").addEventListener("click", async () => {
		const code =
			required<HTMLElement>("[data-export-output]").textContent ?? "";
		try {
			await navigator.clipboard.writeText(code);
			toast("SDK code copied.");
		} catch {
			toast("Clipboard access was denied.", "error");
		}
	});
}
