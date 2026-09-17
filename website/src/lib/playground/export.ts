import type { CompiledRoute } from "qql-wasm-current";
import type { ExportLanguage, PlaygroundSettings } from "./types";

function quotePython(source: string): string {
	return `qql = """${source.replace(/"""/g, '\\"\\"\\"')}"""`;
}

function quoteRust(source: string): string {
	const hashes = source.includes('"#') ? "##" : "#";
	return `r${hashes}"${source}"${hashes}`;
}

export function exportCode(
	language: ExportLanguage,
	source: string,
	conn: PlaygroundSettings,
	route: CompiledRoute | null,
	statementIndex: number,
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
	const payload = JSON.stringify(route.payload, null, 2).replace(/'/g, "'\\''");
	const header = conn.qdrantKey
		? ` \\\n  -H ${JSON.stringify(`api-key: ${conn.qdrantKey}`)}`
		: "";
	return `# Statement ${statementIndex + 1} of ${statementCount} · compiled from editor
curl -X ${route.method} ${JSON.stringify(`${conn.qdrantUrl}${route.path}`)} \\
  -H "content-type: application/json"${header} \\
  --data '${payload}'`;
}
