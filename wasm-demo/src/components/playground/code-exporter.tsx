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
  selectedStmtIndex?: number
}

export function CodeExporter({
  open,
  onOpenChange,
  query,
  qdrantUrl,
  analysis,
  selectedStmtIndex = 0,
}: CodeExporterProps) {
  const [lang, setLang] = useState<"python" | "node" | "rust" | "curl">("python")
  const [copied, setCopied] = useState(false)

  const cleanUrl = qdrantUrl.trim() || "http://localhost:6333"

  const codeSnippets = useMemo(() => {
    const escapedQuery = query.replace(/\\/g, "\\\\").replace(/`/g, "\\`").replace(/"/g, '\\"')
    const pyQuery = query.replace(/\\/g, "\\\\").replace(/"""/g, '\\"""')

    const routes = analysis.routes && analysis.routes.length > 0
      ? analysis.routes
      : analysis.route
        ? [analysis.route]
        : []

    const currentRoute = routes[selectedStmtIndex] ?? routes[0] ?? analysis.route
    const restPath = currentRoute?.path ?? "/collections/berlin_airbnb/points/query"
    const restMethod = (currentRoute?.method ?? "POST").toUpperCase()
    const payloadStr = JSON.stringify(currentRoute?.payload ?? {}, null, 2)

    return {
      python: `# Install pyqql SDK: pip install pyqql
from pyqql import Client

# Connect to Qdrant cluster via pyqql client
client = Client("${cleanUrl}")

# Full script query (multi-statement batching supported)
query_str = """${pyQuery}"""

response = client.execute(query_str)
print(response)`,

      node: `// Install nqql SDK: npm install nqql
import { Client } from 'nqql';

const client = new Client('${cleanUrl}');

// Full script query string
const queryStr = \`${escapedQuery}\`;

async function run() {
  const response = await client.execute(queryStr);
  console.log(JSON.stringify(response, null, 2));
}

run().catch(console.error);`,

      rust: `// Cargo.toml: qql_core = "0.1", qql_plan = "0.1"
use qql_core::parser::Parser;
use qql_plan::routing::route;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let qql = r#"${query}"#;
    let stmts = Parser::parse_all(qql)?;
    
    for (i, stmt) in stmts.iter().enumerate() {
        let r = route(stmt);
        println!("Statement #{}: {} {}", i + 1, r.method, r.path);
    }
    Ok(())
}`,

      curl: `# Compiled REST route for Statement #${selectedStmtIndex + 1} of ${analysis.statements_count || 1}
curl -X ${restMethod} "${cleanUrl}${restPath}" \\
  -H "Content-Type: application/json" \\
  -d '${payloadStr.replace(/'/g, "'\\''")}'`,
    }
  }, [query, cleanUrl, analysis, selectedStmtIndex])

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
            <div className="size-8 rounded-md bg-primary/10 flex items-center justify-center">
              <Code2Icon className="size-4 text-primary" />
            </div>
            <div>
              <DialogTitle className="text-base font-semibold">SDK Code Exporter</DialogTitle>
              <DialogDescription className="text-xs text-muted-foreground">
                {lang === "python" || lang === "node"
                  ? "Export full script execution snippet for Python (pyqql) and Node.js (nqql)."
                  : `Export compiled REST route or Rust AST parser loop for Statement #${selectedStmtIndex + 1} of ${analysis.statements_count || 1}.`}
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        <Tabs
          value={lang}
          onValueChange={(v) => setLang(v as typeof lang)}
          className="flex-1 min-h-0 flex flex-col pt-3"
        >
          <div className="flex items-center justify-between gap-4 pb-3">
            <TabsList className="grid grid-cols-4 w-[380px]">
              <TabsTrigger value="python" className="text-xs font-mono">Python</TabsTrigger>
              <TabsTrigger value="node" className="text-xs font-mono">Node.js</TabsTrigger>
              <TabsTrigger value="rust" className="text-xs font-mono">Rust</TabsTrigger>
              <TabsTrigger value="curl" className="text-xs font-mono">cURL</TabsTrigger>
            </TabsList>

            <Button
              variant="outline"
              size="sm"
              onClick={handleCopy}
              className="font-mono text-xs gap-1.5 shrink-0"
            >
              {copied ? (
                <CheckIcon className="size-3.5 text-emerald-500" />
              ) : (
                <CopyIcon className="size-3.5" />
              )}
              {copied ? "Copied Snippet" : "Copy Code"}
            </Button>
          </div>

          <div className="flex-1 min-h-0 relative border rounded-md overflow-hidden bg-muted/40">
            <TabsContent value="python" className="m-0 h-full">
              <pre className="p-4 font-mono text-xs overflow-auto h-full text-foreground leading-relaxed">
                {codeSnippets.python}
              </pre>
            </TabsContent>

            <TabsContent value="node" className="m-0 h-full">
              <pre className="p-4 font-mono text-xs overflow-auto h-full text-foreground leading-relaxed">
                {codeSnippets.node}
              </pre>
            </TabsContent>

            <TabsContent value="rust" className="m-0 h-full">
              <pre className="p-4 font-mono text-xs overflow-auto h-full text-foreground leading-relaxed">
                {codeSnippets.rust}
              </pre>
            </TabsContent>

            <TabsContent value="curl" className="m-0 h-full">
              <pre className="p-4 font-mono text-xs overflow-auto h-full text-foreground leading-relaxed">
                {codeSnippets.curl}
              </pre>
            </TabsContent>
          </div>
        </Tabs>
      </DialogContent>
    </Dialog>
  )
}
