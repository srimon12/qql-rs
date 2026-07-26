import { Client, HttpEmbedder, injectFilter } from '../../crates/nqql/index.js';

// 1. First-class custom embedding provider (Ollama / vLLM / OpenAI)
const embedder = new HttpEmbedder({
    endpoint: "http://localhost:11434/v1/embeddings",
    model: "nomic-embed-text",
    dimension: 768,
    apiKey: "optional-key"
});

const client = new Client({
    url: "http://localhost:6333",
    apiKey: "qdrant-secret",
    useGrpc: false,
    embedder: embedder
});

// 2. Multi-tenant security gateway function
function enforceSecurityGateway(userRole, tenantId, query) {
    let secured = injectFilter(query, "tenant_id", "=", `"${tenantId}"`);
    return secured;
}

// 3. Hybrid search query
const rawQuery = "QUERY 'transformers attention mechanism' FROM research_papers LIMIT 5 USING HYBRID";

const ast = enforceSecurityGateway("viewer", "lab-beta", rawQuery);

console.log("=== Injected AST Filter ===");
console.log(ast.Query.query_filter);

console.log("\n=== Secured Execution Plan ===");
const plan = client.explain(rawQuery);
console.log(plan);
