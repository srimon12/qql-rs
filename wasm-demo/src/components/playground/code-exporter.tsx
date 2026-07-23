import { useState, useMemo } from "react"
import { Code2Icon, CopyIcon, CheckIcon } from "lucide-react"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import type { AnalysisResult } from "@/lib/qql-types"

type CodeExporterProps = {
  open: boolean
  onOpenChange: (open: boolean) => void
  query: string
  qdrantUrl: string
  analysis: AnalysisResult
}

export function CodeExporter({
  open,
  onOpenChange,
  query,
  qdrantUrl,
  analysis,
}: CodeExporterProps) {
  const [lang, setLang] = useState<"python" | "node" | "rust" | "curl">("python")
  const [copied, setCopied] = useState(false)

  const cleanUrl = qdrantUrl.trim() || "http://localhost:6333"

  const codeSnippets = useMemo(() => {
    const escapedQuery = query.replace(/\\/g, "\\\\").replace(/`/g, "\\`").replace(/"/g, '\\"')
    const pyQuery = query.replace(/\\/g, "\\\\").replace(/"""/g, '\\"""')

    const restPath = analysis.route?.path ?? "/collections/sec10k/points/query"
    const restMethod = (analysis.route?.method ?? "POST").toUpperCase()
    const payloadStr = JSON.stringify(analysis.route?.payload ?? {}, null, 2)

    return {
      python: `# Install pyqql SDK: pip install pyqql
from pyqql import Client

client = Client("${cleanUrl}")

query_str = """${pyQuery}"""

# Execute query against Qdrant cluster
response = client.execute(query_str)
print(response)`,

      node: `// Install nqql SDK: npm install nqql
import { Client } from 'nqql';

const client = new Client('${cleanUrl}');

const queryStr = \`${escapedQuery}\`;

async function run() {
  const response = await client.execute(queryStr);
  console.log(JSON.stringify(response, null, 2));
}

run().catch(console.error);`,

      rust: `// Cargo.toml: qql = "0.1"
use qql::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new("${cleanUrl}", None);
    let query_str = r#"${query}"#;

    let response = client.execute(query_str).await?;
    println!("{:#?}", response);
    Ok(())
}`,

      curl: `# Compiled REST route from QQL WASM planner
curl -X ${restMethod} "${cleanUrl}${restPath}" \\
  -H "Content-Type: application/json" \\
  -d '${payloadStr.replace(/'/g, "'\\''")}'`,
    }
  }, [query, cleanUrl, analysis])

  const activeSnippet = codeSnippets[lang]

  const handleCopy = () => {
    navigator.clipboard.writeText(activeSnippet)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[900px] sm:w-[90vw] max-w-3xl max-h-[85vh] flex flex-col overflow-hidden p-6">
        <DialogHeader className="pb-2 border-b">
          <div className="flex items-center gap-2">
            <Code2Icon className="size-5 text-primary" />
            <DialogTitle className="text-lg font-semibold">
              Copy as Production SDK Code
            </DialogTitle>
          </div>
          <DialogDescription className="text-xs text-muted-foreground">
            Export this QQL statement into native SDK calls for Python, Node.js, Rust, or raw cURL.
          </DialogDescription>
        </DialogHeader>

        <Tabs
          value={lang}
          onValueChange={(v) => setLang(v as "python" | "node" | "rust" | "curl")}
          className="flex-1 min-h-0 flex flex-col pt-2"
        >
          <TabsList className="w-full justify-start font-mono text-xs">
            <TabsTrigger value="python">Python (pyqql)</TabsTrigger>
            <TabsTrigger value="node">Node.js (nqql)</TabsTrigger>
            <TabsTrigger value="rust">Rust (qql)</TabsTrigger>
            <TabsTrigger value="curl">cURL REST</TabsTrigger>
          </TabsList>

          {Object.entries(codeSnippets).map(([key, code]) => (
            <TabsContent key={key} value={key} className="flex-1 min-h-0 pt-3">
              <div className="h-full rounded-lg border bg-card p-3 font-mono text-xs overflow-auto leading-relaxed">
                <pre className="whitespace-pre-wrap">{code}</pre>
              </div>
            </TabsContent>
          ))}
        </Tabs>

        <div className="flex items-center justify-between border-t pt-3 mt-3">
          <span className="text-[11px] font-mono text-muted-foreground">
            Endpoint: {cleanUrl}
          </span>
          <div className="flex items-center gap-2">
            <Button variant="ghost" size="sm" onClick={() => onOpenChange(false)}>
              Close
            </Button>
            <Button size="sm" onClick={handleCopy} className="gap-1.5 font-mono text-xs">
              {copied ? <CheckIcon className="size-3.5 text-emerald-500" /> : <CopyIcon className="size-3.5" />}
              {copied ? "Copied to Clipboard!" : `Copy ${lang.toUpperCase()} Code`}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
